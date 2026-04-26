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

    /// Plain-language rule (stored in report)
    #[arg(long = "rule", alias = "cut-rule")]
    cut_rule: Option<String>,

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
    dry_run: bool,
    scanned_count: usize,
    matched_count: usize,
    moved_count: usize,
    cut_dir: String,
    complexity: ComplexityEstimate,
    entries: Vec<ReportEntry>,
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

    // discover images
    let images = discover_images(&target, &args.cut_dir)?;

    // complexity estimate (deterministic, no money)
    let complexity = estimate_complexity(images.len(), args.vision);
    eprintln!(
        "Complexity estimate: score={} tier={} ({} images)",
        complexity.score,
        complexity.tier,
        images.len()
    );

    // Optional: call python vision backend to get labels/safesearch/text
    let vision_map = if args.vision {
        // Keep it simple: label + safe_search for the initial example.
        let req = VisionRequest {
            images: images
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            features: vec!["LABEL_DETECTION".into(), "SAFE_SEARCH_DETECTION".into()],
        };
        Some(call_python_vision(&req)?)
    } else {
        None
    };

    let cut_dir_path = target.join(&args.cut_dir);
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
            if rule.trim().eq_ignore_ascii_case("screenshots") {
                if is_screenshot_name(img) {
                    reasons.push("screenshot_pattern".to_string());
                }
            }
            if rule.trim().eq_ignore_ascii_case("duplicates") {
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
            }
            if rule.trim().eq_ignore_ascii_case("explicit") && args.vision {
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
        dry_run: args.dry_run,
        scanned_count: images.len(),
        matched_count: matched.len(),
        moved_count,
        cut_dir: cut_dir_path.to_string_lossy().to_string(),
        complexity,
        entries,
    };

    write_report(&report_path, &report)?;
    eprintln!("Wrote report: {}", report_path.display());
    Ok(())
}

fn discover_images(root: &Path, cut_dir_name: &str) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let cut_dir = root.join(cut_dir_name);
    let exts: HashSet<&'static str> = ["jpg", "jpeg", "png", "webp", "heic", "tiff", "gif"]
        .into_iter()
        .collect();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if path == cut_dir || path.starts_with(&cut_dir) {
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
    Ok(out)
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

fn call_python_vision(req: &VisionRequest) -> Result<HashMap<String, VisionImageResult>> {
    let mut cmd = Command::new("python3");
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
