use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(
    name = "caesim",
    version,
    about = "Safe image-library trimming utility"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan a folder and move matched images into a cut folder
    Cut(CutArgs),
}

#[derive(Parser, Debug)]
struct CutArgs {
    /// Root folder to scan
    path: PathBuf,

    /// Local rule (for example: screenshots, duplicates, explicit, landscape, portrait)
    #[arg(long = "rule", alias = "cut-rule")]
    cut_rule: Option<String>,

    /// Vision label query like "food" or "receipt"
    #[arg(long = "contains")]
    contains: Option<String>,

    /// Folder to move matched files into (defaults to <path>/cut)
    #[arg(long = "destination")]
    destination: Option<PathBuf>,

    /// Filter out all images "containing" the given image (stored in report)
    /// (placeholder; not implemented in MVP)
    #[arg(long = "cut-img")]
    cut_img: Option<PathBuf>,

    /// Preview matches without moving files
    #[arg(long)]
    dry_run: bool,

    /// Cut folder name (default: cut)
    #[arg(long = "cut-dir", default_value = "cut")]
    cut_dir: String,

    /// Report JSON path (default: .caesim-report.json in target folder)
    #[arg(long)]
    report: Option<PathBuf>,

    /// Enable optional Python + Google Vision backend (post-run/advanced rules)
    #[arg(long)]
    vision: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct ReportEntry {
    source: String,
    destination: Option<String>,
    reason: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct RunReport {
    run_id: String,
    target_path: String,
    rule: Option<String>,
    contains: Option<String>,
    destination: Option<String>,
    dry_run: bool,
    scanned_count: usize,
    matched_count: usize,
    moved_count: usize,
    cut_dir: String,
    complexity: ComplexityEstimate,
    entries: Vec<ReportEntry>,
    vision_errors: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ComplexityEstimate {
    score: u64,
    tier: String,
    notes: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct VisionRequest {
    images: Vec<String>,
    features: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct VisionImageResult {
    path: String,
    labels: Vec<String>,
    safe_search: Option<HashMap<String, String>>,
    text: Option<String>,
    web_full_matches: Vec<String>,
    web_partial_matches: Vec<String>,
    web_best_guess_labels: Vec<String>,
    dominant_colors: Vec<String>,
    errors: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct VisionResponse {
    results: Vec<VisionImageResult>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Cut(args) => run_cut(args),
    }
}

fn run_cut(args: CutArgs) -> Result<()> {
    if !args.path.exists() {
        return Err(anyhow!("path does not exist: {}", args.path.display()));
    }
    if !args.path.is_dir() {
        return Err(anyhow!("path is not a directory: {}", args.path.display()));
    }

    let target = fs::canonicalize(&args.path)
        .with_context(|| format!("failed to canonicalize {}", args.path.display()))?;

    // resolve destination and discover images
    let cut_dir_path = resolve_destination(&target, args.destination.as_ref(), &args.cut_dir)?;
    let mut skip_dirs = vec![target.join(&args.cut_dir)];
    if cut_dir_path.starts_with(&target) {
        skip_dirs.push(cut_dir_path.clone());
    }
    let images = discover_images(&target, &skip_dirs)?;
    let image_paths: HashSet<PathBuf> = images.iter().cloned().collect();

    // complexity estimate (deterministic, no money)
    let complexity = estimate_complexity(images.len(), args.vision);
    eprintln!(
        "Complexity estimate: score={} tier={} ({} images)",
        complexity.score,
        complexity.tier,
        images.len()
    );

    let mut vision_features = args
        .cut_rule
        .as_deref()
        .map(vision_features_for_rule)
        .unwrap_or_else(Vec::new);
    if args.contains.is_some() && !vision_features.iter().any(|f| f == "LABEL_DETECTION") {
        vision_features.push("LABEL_DETECTION".to_string());
    }

    // Optional: call python vision backend to get labels/safesearch/text/web/image properties.
    let vision_map = if args.vision {
        eprintln!(
            "Running Google Vision analysis with {} feature(s): {}",
            vision_features.len(),
            vision_features.join(",")
        );
        let req = VisionRequest {
            images: images
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            features: vision_features.clone(),
        };
        Some(call_python_vision(&req)?)
    } else {
        None
    };
    let vision_duplicate_sources = vision_map
        .as_ref()
        .map(|vm| build_vision_duplicate_sources(&images, vm))
        .unwrap_or_default();

    let report_path = args
        .report
        .clone()
        .unwrap_or_else(|| target.join(".caesim-report.json"));

    // score / match
    let mut entries: Vec<ReportEntry> = Vec::new();
    let mut matched: Vec<PathBuf> = Vec::new();

    // duplicate detection by sha256 (simple but deterministic)
    let mut seen_hashes: HashMap<String, PathBuf> = HashMap::new();

    for img in &images {
        let mut reasons: Vec<String> = Vec::new();

        if let Some(rule) = args.cut_rule.as_deref() {
            let normalized_rule = rule.trim().to_ascii_lowercase();
            if normalized_rule == "screenshots" {
                if is_screenshot_name(img) {
                    reasons.push("screenshot_pattern".to_string());
                }
            }
            if normalized_rule == "duplicates" {
                if let Ok(hash) = sha256_file(img) {
                    if let Some(first_path) = seen_hashes.get(&hash) {
                        reasons.push(format!(
                            "duplicate_sha256_of:{}",
                            first_path.to_string_lossy()
                        ));
                    } else {
                        seen_hashes.insert(hash, img.clone());
                    }
                }
                if let Some(original_path) = duplicate_copy_source(img, &image_paths) {
                    reasons.push(format!(
                        "duplicate_filename_copy_of:{}",
                        original_path.to_string_lossy()
                    ));
                }
                if let Some((original_path, signal)) = vision_duplicate_sources.get(img) {
                    reasons.push(format!(
                        "vision_duplicate_{signal}_of:{}",
                        original_path.to_string_lossy()
                    ));
                }
            }
            if normalized_rule == "explicit" && args.vision {
                if let Some(vm) = &vision_map {
                    if let Some(vr) = vm.get(&img.to_string_lossy().to_string()) {
                        if let Some(ss) = &vr.safe_search {
                            // Extremely conservative: if any of these are VERY_LIKELY/LIKELY we flag it.
                            let flagged = ["adult", "violence", "racy"].iter().any(|k| {
                                ss.get(*k)
                                    .map(|v| v == "VERY_LIKELY" || v == "LIKELY")
                                    .unwrap_or(false)
                            });
                            if flagged {
                                reasons.push("vision_safe_search_flag".to_string());
                            }
                        }
                    }
                }
            }
            if normalized_rule == "landscape" || normalized_rule == "portrait" {
                if let Ok(true) = image_matches_orientation(img, &normalized_rule) {
                    reasons.push(format!("orientation_{normalized_rule}"));
                }
            }
        }

        if args.vision {
            if let Some(query) = args.contains.as_deref() {
                if let Some(vm) = &vision_map {
                    if let Some(vr) = vm.get(&img.to_string_lossy().to_string()) {
                        let matched_labels = matching_vision_labels(query, &vr.labels);
                        if !matched_labels.is_empty() {
                            reasons.push(format!(
                                "vision_contains_match:{}",
                                matched_labels.join("|")
                            ));
                        }
                    }
                }
            }
        }

        if !reasons.is_empty() {
            matched.push(img.clone());
            entries.push(ReportEntry {
                source: img.to_string_lossy().to_string(),
                destination: None,
                reason: reasons.join(","),
            });
        }
    }

    // move (unless dry-run)
    let mut moved_count = 0usize;
    if !args.dry_run {
        fs::create_dir_all(&cut_dir_path)
            .with_context(|| format!("failed to create {}", cut_dir_path.display()))?;

        for (idx, src) in matched.iter().enumerate() {
            let dest = unique_destination(&cut_dir_path, src)?;
            fs::rename(src, &dest).with_context(|| {
                format!("failed to move {} -> {}", src.display(), dest.display())
            })?;
            moved_count += 1;

            // update entry destination (same order as matched)
            if let Some(entry) = entries.get_mut(idx) {
                entry.destination = Some(dest.to_string_lossy().to_string());
            }
        }
    }

    let report = RunReport {
        run_id: chrono_run_id(),
        target_path: target.to_string_lossy().to_string(),
        rule: args.cut_rule.clone(),
        contains: args.contains.clone(),
        destination: Some(cut_dir_path.to_string_lossy().to_string()),
        dry_run: args.dry_run,
        scanned_count: images.len(),
        matched_count: matched.len(),
        moved_count,
        cut_dir: cut_dir_path.to_string_lossy().to_string(),
        complexity,
        entries,
        vision_errors: {
            let mut v: Vec<String> = Vec::new();
            if let Some(vm) = vision_map.as_ref() {
                for (p, vr) in vm.iter() {
                    for err in &vr.errors {
                        if !err.is_empty() {
                            v.push(format!("{}: {}", p, err));
                        }
                    }
                }
            }
            v
        },
    };

    write_report(&report_path, &report)?;
    if args.dry_run {
        eprintln!(
            "Matched {} image(s); dry run, moved 0.",
            report.matched_count
        );
    } else {
        eprintln!(
            "Matched {} image(s); moved {} into {}.",
            report.matched_count,
            report.moved_count,
            cut_dir_path.display()
        );
    }
    eprintln!("Wrote report: {}", report_path.display());
    Ok(())
}

fn discover_images(root: &Path, skip_dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let exts: HashSet<&'static str> = [
        "jpg", "jpeg", "png", "webp", "heic", "tiff", "gif", "avif", "svg",
    ]
    .into_iter()
    .collect();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if skip_dirs.iter().any(|skip| path == skip || path.starts_with(skip)) {
            continue;
        }
        if entry.file_type().is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let lower = ext.to_ascii_lowercase();
                if exts.contains(lower.as_str()) {
                    out.push(path.to_path_buf());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn resolve_destination(
    target: &Path,
    destination: Option<&PathBuf>,
    cut_dir: &str,
) -> Result<PathBuf> {
    match destination {
        Some(dest) if dest.is_absolute() => Ok(dest.clone()),
        Some(dest) => Ok(std::env::current_dir()
            .context("failed to read current directory")?
            .join(dest)),
        None => Ok(target.join(cut_dir)),
    }
}

fn is_screenshot_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    name.contains("screenshot") || name.starts_with("screen shot") || name.starts_with("img_")
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn duplicate_copy_source(path: &Path, image_paths: &HashSet<PathBuf>) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let original_stem = strip_duplicate_copy_suffix(stem)?;
    let extension = path.extension().and_then(|e| e.to_str());
    let original_filename = match extension {
        Some(ext) if !ext.is_empty() => format!("{original_stem}.{ext}"),
        _ => original_stem.to_string(),
    };
    let original_path = path.with_file_name(original_filename);

    if original_path != path && image_paths.contains(&original_path) {
        Some(original_path)
    } else {
        None
    }
}

fn build_vision_duplicate_sources(
    images: &[PathBuf],
    vision_map: &HashMap<String, VisionImageResult>,
) -> HashMap<PathBuf, (PathBuf, String)> {
    let mut seen: HashMap<String, PathBuf> = HashMap::new();
    let mut duplicates: HashMap<PathBuf, (PathBuf, String)> = HashMap::new();

    for image in images {
        let image_key = image.to_string_lossy().to_string();
        let Some(result) = vision_map.get(&image_key) else {
            continue;
        };

        for (key, signal) in vision_duplicate_keys(result) {
            if let Some(first_path) = seen.get(&key) {
                duplicates
                    .entry(image.clone())
                    .or_insert_with(|| (first_path.clone(), signal));
            } else {
                seen.insert(key, image.clone());
            }
        }
    }

    duplicates
}

fn vision_duplicate_keys(result: &VisionImageResult) -> Vec<(String, String)> {
    let mut keys = Vec::new();

    for url in &result.web_full_matches {
        if !url.trim().is_empty() {
            keys.push((
                format!("web-full:{}", url.trim()),
                "web_full_match".to_string(),
            ));
        }
    }

    for url in &result.web_partial_matches {
        if !url.trim().is_empty() {
            keys.push((
                format!("web-partial:{}", url.trim()),
                "web_partial_match".to_string(),
            ));
        }
    }

    if let Some(text_key) = normalized_text_duplicate_key(result.text.as_deref()) {
        keys.push((format!("ocr:{text_key}"), "ocr_text_match".to_string()));
    }

    if result.labels.len() >= 4 && result.dominant_colors.len() >= 3 {
        let labels = result
            .labels
            .iter()
            .take(6)
            .map(|label| label.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("|");
        let colors = result
            .dominant_colors
            .iter()
            .take(4)
            .map(|color| color.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("|");
        if !labels.is_empty() && !colors.is_empty() {
            keys.push((
                format!("signature:{labels}:{colors}"),
                "signature_match".to_string(),
            ));
        }
    }

    keys
}

fn normalized_text_duplicate_key(text: Option<&str>) -> Option<String> {
    let normalized = text?
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_whitespace() {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.len() >= 32 {
        Some(normalized)
    } else {
        None
    }
}

fn strip_duplicate_copy_suffix(stem: &str) -> Option<&str> {
    if let Some(before_suffix) = stem.strip_suffix(" copy") {
        return non_empty_trimmed(before_suffix);
    }
    if let Some(before_suffix) = stem.strip_suffix("- copy") {
        return non_empty_trimmed(before_suffix);
    }
    if let Some(before_suffix) = stem.strip_suffix("_copy") {
        return non_empty_trimmed(before_suffix);
    }

    let (before_close, _) = stem.rsplit_once(')')?;
    let (before_open, number) = before_close.rsplit_once('(')?;
    if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
        return non_empty_trimmed(before_open);
    }

    None
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim_end_matches([' ', '-', '_']);
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn unique_destination(cut_dir: &Path, src: &Path) -> Result<PathBuf> {
    let filename = src
        .file_name()
        .ok_or_else(|| anyhow!("missing filename: {}", src.display()))?;
    let mut dest = cut_dir.join(filename);
    if !dest.exists() {
        return Ok(dest);
    }
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
    for i in 1..10_000u32 {
        let candidate = if ext.is_empty() {
            cut_dir.join(format!("{stem}_{i}"))
        } else {
            cut_dir.join(format!("{stem}_{i}.{ext}"))
        };
        if !candidate.exists() {
            dest = candidate;
            break;
        }
    }
    Ok(dest)
}

fn write_report(path: &Path, report: &RunReport) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    let mut f = fs::File::create(path)?;
    f.write_all(json.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

fn chrono_run_id() -> String {
    // Avoid adding chrono dependency for now; keep deterministic-ish timestamp via system time.
    // Format: seconds since epoch as string.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn estimate_complexity(image_count: usize, vision_enabled: bool) -> ComplexityEstimate {
    let mut notes = Vec::new();
    let mut score = image_count as u64;
    if vision_enabled {
        // crude multiplier: each image uses 2 features in our request
        score = score.saturating_mul(3);
        notes.push("vision_enabled=true (adds remote vision calls)".to_string());
        notes.push("estimated_units ~= images * features".to_string());
    } else {
        notes.push("vision_enabled=false (local heuristics only)".to_string());
    }

    let tier = match score {
        0..=5_000 => "low",
        5_001..=50_000 => "medium",
        _ => "high",
    }
    .to_string();

    ComplexityEstimate { score, tier, notes }
}

fn vision_features_for_rule(rule: &str) -> Vec<String> {
    let normalized = rule.trim().to_ascii_lowercase();
    if normalized == "duplicates" {
        return vec![
            "LABEL_DETECTION".into(),
            "TEXT_DETECTION".into(),
            "WEB_DETECTION".into(),
            "IMAGE_PROPERTIES".into(),
        ];
    }
    if normalized == "explicit" {
        return vec!["SAFE_SEARCH_DETECTION".into()];
    }
    Vec::new()
}

fn image_matches_orientation(path: &Path, rule: &str) -> Result<bool> {
    let (width, height) = image::image_dimensions(path)
        .with_context(|| format!("failed to read image dimensions for {}", path.display()))?;
    Ok(orientation_matches_dimensions(width, height, rule))
}

fn orientation_matches_dimensions(width: u32, height: u32, rule: &str) -> bool {
    match rule.trim().to_ascii_lowercase().as_str() {
        "landscape" => width > height,
        "portrait" => height > width,
        _ => false,
    }
}

fn matching_vision_labels(query: &str, labels: &[String]) -> Vec<String> {
    let query_tokens = normalized_words(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }

    labels
        .iter()
        .filter(|label| {
            let label_tokens = normalized_words(label);
            query_tokens.iter().all(|query_token| {
                label_tokens
                    .iter()
                    .any(|label_token| label_token == query_token)
            })
        })
        .cloned()
        .collect()
}

fn normalized_words(value: &str) -> Vec<String> {
    value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn call_python_vision(req: &VisionRequest) -> Result<HashMap<String, VisionImageResult>> {
    let mut cmd = Command::new(find_python()?);
    cmd.arg(find_vision_backend()?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = cmd
        .spawn()
        .context("failed to start python vision backend")?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open python stdin"))?;
        let payload = serde_json::to_vec(req)?;
        stdin.write_all(&payload)?;
    }

    let output = child
        .wait_with_output()
        .context("failed waiting for python vision backend")?;
    if !output.status.success() {
        return Err(anyhow!(
            "python vision backend failed with exit code {:?}",
            output.status.code()
        ));
    }

    let resp: VisionResponse = serde_json::from_slice(&output.stdout)
        .context("failed to parse python vision backend output")?;
    Ok(resp
        .results
        .into_iter()
        .map(|r| (r.path.clone(), r))
        .collect())
}

fn find_python() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CAESIM_PYTHON") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
        return Err(anyhow!("CAESIM_PYTHON does not exist: {}", path.display()));
    }

    for candidate in [PathBuf::from(".venv/bin/python"), PathBuf::from("python3")] {
        if candidate.exists() || candidate.as_os_str() == "python3" {
            return Ok(candidate);
        }
    }

    Ok(PathBuf::from("python3"))
}

fn find_vision_backend() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CAESIM_VISION_BACKEND") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
        return Err(anyhow!(
            "CAESIM_VISION_BACKEND does not exist: {}",
            path.display()
        ));
    }

    let mut candidates = vec![PathBuf::from("python/vision_backend.py")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("python/vision_backend.py"));
            candidates.push(dir.join("../python/vision_backend.py"));
        }
    }

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            anyhow!(
                "could not find python/vision_backend.py; set CAESIM_VISION_BACKEND to its path"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_numbered_duplicate_copy_names() {
        assert_eq!(strip_duplicate_copy_suffix("penrose (1)"), Some("penrose"));
        assert_eq!(strip_duplicate_copy_suffix("penrose (12)"), Some("penrose"));
        assert_eq!(strip_duplicate_copy_suffix("penrose copy"), Some("penrose"));
        assert_eq!(
            strip_duplicate_copy_suffix("penrose- copy"),
            Some("penrose")
        );
        assert_eq!(strip_duplicate_copy_suffix("penrose_copy"), Some("penrose"));
        assert_eq!(strip_duplicate_copy_suffix("penrose (final)"), None);
    }

    #[test]
    fn finds_original_path_for_duplicate_copy_name() {
        let root = PathBuf::from("/tmp/caesim-test");
        let original = root.join("penrose.svg");
        let copy = root.join("penrose (1).svg");
        let paths = HashSet::from([original.clone(), copy.clone()]);

        assert_eq!(duplicate_copy_source(&copy, &paths), Some(original));
        assert_eq!(
            duplicate_copy_source(&root.join("missing (1).svg"), &paths),
            None
        );
    }

    #[test]
    fn detects_vision_duplicate_from_shared_full_web_match() {
        let first = PathBuf::from("/tmp/caesim-test/first.jpg");
        let second = PathBuf::from("/tmp/caesim-test/second.jpg");
        let images = vec![first.clone(), second.clone()];
        let vision_map = HashMap::from([
            (
                first.to_string_lossy().to_string(),
                vision_result(
                    &first,
                    vec!["https://example.com/same-image.jpg"],
                    vec![],
                    None,
                ),
            ),
            (
                second.to_string_lossy().to_string(),
                vision_result(
                    &second,
                    vec!["https://example.com/same-image.jpg"],
                    vec![],
                    None,
                ),
            ),
        ]);

        let duplicates = build_vision_duplicate_sources(&images, &vision_map);
        assert_eq!(
            duplicates.get(&second),
            Some(&(first, "web_full_match".to_string()))
        );
    }

    #[test]
    fn normalizes_long_ocr_text_for_duplicate_detection() {
        assert_eq!(
            normalized_text_duplicate_key(Some("Invoice #123\nTotal: EUR 42.00, thank you")),
            Some("invoice 123 total eur 4200 thank you".to_string())
        );
        assert_eq!(normalized_text_duplicate_key(Some("too short")), None);
    }

    #[test]
    fn rule_features_cover_only_special_cases() {
        assert_eq!(vision_features_for_rule("food"), Vec::<String>::new());
        assert_eq!(vision_features_for_rule("screenshots"), Vec::<String>::new());
        assert_eq!(vision_features_for_rule("duplicates"), vec![
            "LABEL_DETECTION".to_string(),
            "TEXT_DETECTION".to_string(),
            "WEB_DETECTION".to_string(),
            "IMAGE_PROPERTIES".to_string(),
        ]);
        assert_eq!(
            vision_features_for_rule("explicit"),
            vec!["SAFE_SEARCH_DETECTION"]
        );
    }

    #[test]
    fn matches_contains_query_against_vision_labels() {
        let labels = vec![
            "Food".to_string(),
            "Fast food".to_string(),
            "Tableware".to_string(),
        ];

        assert_eq!(
            matching_vision_labels("food", &labels),
            vec!["Food".to_string(), "Fast food".to_string()]
        );
        assert_eq!(
            matching_vision_labels("fast food", &labels),
            vec!["Fast food".to_string()]
        );
        assert!(matching_vision_labels("receipt", &labels).is_empty());
    }

    #[test]
    fn matches_orientation_by_dimensions() {
        assert!(orientation_matches_dimensions(1920, 1080, "landscape"));
        assert!(!orientation_matches_dimensions(1080, 1920, "landscape"));
        assert!(orientation_matches_dimensions(1080, 1920, "portrait"));
        assert!(!orientation_matches_dimensions(1920, 1080, "portrait"));
        assert!(!orientation_matches_dimensions(1080, 1080, "landscape"));
        assert!(!orientation_matches_dimensions(1080, 1080, "portrait"));
    }

    fn vision_result(
        path: &Path,
        web_full_matches: Vec<&str>,
        web_partial_matches: Vec<&str>,
        text: Option<&str>,
    ) -> VisionImageResult {
        VisionImageResult {
            path: path.to_string_lossy().to_string(),
            labels: Vec::new(),
            safe_search: None,
            text: text.map(str::to_string),
            web_full_matches: web_full_matches.into_iter().map(str::to_string).collect(),
            web_partial_matches: web_partial_matches
                .into_iter()
                .map(str::to_string)
                .collect(),
            web_best_guess_labels: Vec::new(),
            dominant_colors: Vec::new(),
            errors: Vec::new(),
        }
    }
}
