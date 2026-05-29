use anyhow::{anyhow, Context, Result};
mod ai_assist;
mod auth;
use clap::{Parser, Subcommand};
use std::env;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;
use tokio::runtime::Runtime;

#[derive(Parser, Debug)]
#[command(
    name = "caesim",
    version,
    about = "Safe image-library trimming utility"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan a folder and move matched images into a cut folder
    Cut(CutArgs),
    /// AI assistant for generating caesim commands delivered by backboard.io
    #[command(name = "ai-assist")]
    AiAssist,
    /// Configure and learn about Google Vision setup
    Vision(VisionArgs),
    /// Create a local account session with one-time password signup
    #[command(alias = "register")]
    Signup(SignupArgs),
    /// Sign in to an existing local account session with password
    Login(LoginArgs),
    /// Show the active local account session
    Whoami,
    /// Change account password
    ChangePassword(ChangePasswordArgs),
    /// Sign out and delete local session
    Logout,
    /// Manage account credits
    Credits(CreditsArgs),
}

#[derive(Parser, Debug)]
struct CutArgs {
    /// Root folder to scan
    path: PathBuf,

    /// Local rule (for example: screenshots, duplicates, explicit, landscape, portrait)
    #[arg(long = "rule", alias = "cut-rule")]
    cut_rule: Option<String>,

    /// Vision label query like "food" or "receipt"
    /// Use `--find cars` to enable vision mode and search for the label.
    #[arg(long = "find", value_name = "LABEL")]
    find: Option<String>,

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


}

#[derive(Parser, Debug)]
struct VisionArgs {
    /// Optional: path to custom service account JSON file to use
    #[arg(long = "service-account-json")]
    service_account_json: Option<PathBuf>,
}

#[derive(Parser, Debug)]
struct LoginArgs {
    /// Email address to use for sign-in
    #[arg(long)]
    email: Option<String>,

    /// Use OTP login flow instead of password login
    #[arg(long)]
    otp: bool,

    /// Verification code from the email sent by Supabase
    #[arg(long = "verification-code")]
    verification_code: Option<String>,
}

#[derive(Parser, Debug)]
struct SignupArgs {
    /// Email address to register
    #[arg(long)]
    email: Option<String>,
}

#[derive(Parser, Debug)]
struct ChangePasswordArgs {
    /// Use OTP reset flow instead of current-password flow
    #[arg(long)]
    otp: bool,
}

#[derive(Parser, Debug)]
struct CreditsArgs {
    /// Check current credit balance
    #[command(subcommand)]
    command: Option<CreditsCommand>,
}

#[derive(Subcommand, Debug)]
enum CreditsCommand {
    /// Show current credit balance
    Balance,
    /// Add toy credits to account
    Add {
        /// Amount in credits (e.g., 100)
        #[arg(long)]
        amount: i64,
    },
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
    find: Option<String>,
    destination: Option<String>,
    dry_run: bool,
    scanned_count: usize,
    matched_count: usize,
    moved_count: usize,
    cut_dir: String,
    complexity: ComplexityEstimate,
    vision_credit_cost: Option<i64>,
    credit_balance_before: Option<i64>,
    credit_balance_after: Option<i64>,
    entries: Vec<ReportEntry>,
    vision_errors: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ComplexityEstimate {
    score: u64,
    tier: String,
    notes: Vec<String>,
}

// Backboard request/response types moved to `src/ai_assist` module.

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
    let _ = dotenvy::dotenv();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if is_missing_cut_rule_value_error(&err.to_string()) {
                print_available_cut_rules();
                return Ok(());
            }

            return Err(err.into());
        }
    };

    match cli.command {
        Some(Commands::Cut(args)) => {
            if is_cut_undo_request(&args) {
                run_cut_undo(args.report.clone())
            } else {
                run_cut(args)
            }
        }
        Some(Commands::AiAssist) => run_assist(),
        Some(Commands::Vision(args)) => run_vision(args),
        Some(Commands::Signup(args)) => run_signup(args),
        Some(Commands::Login(args)) => run_login(args),
        Some(Commands::Whoami) => run_whoami(),
        Some(Commands::ChangePassword(args)) => run_change_password(args),
        Some(Commands::Logout) => run_logout(),
        Some(Commands::Credits(args)) => run_credits(args),
        None => Err(anyhow!("no command provided; run `caesim --help` to list available commands")),
    }
}

fn print_available_cut_rules() {
    println!("Available cut rules:");
    for rule in available_cut_rules() {
        println!("  - {rule}");
    }
    println!();
    println!("Example:");
    println!("  caesim cut <path> --rule duplicates --dry-run");
}

fn available_cut_rules() -> &'static [&'static str] {
    &["screenshots", "duplicates", "explicit", "landscape", "portrait"]
}

fn is_missing_cut_rule_value_error(message: &str) -> bool {
    message.contains("a value is required for '--rule <CUT_RULE>'")
        && message.contains("but none was supplied")
}

fn ensure_signed_out_for_auth(command_name: &str) -> Result<()> {
    if let Some(session) = auth::load_session()? {
        return Err(anyhow!(
            "already signed in as {} ({}); run `caesim logout` before `caesim {}`",
            session.email,
            session.user_id,
            command_name
        ));
    }
    Ok(())
}

async fn complete_otp_session(
    supabase_url: &str,
    supabase_key: &str,
    email: &str,
    code: &str,
    account_created: bool,
) -> Result<()> {
    let session = auth::verify_login(supabase_url, supabase_key, email, code).await?;
    auth::save_session(&auth::StoredSession {
        supabase_url: supabase_url.to_string(),
        user_id: session.user_id.clone(),
        email: session.email.clone(),
        session_token: session.session_token.clone(),
        refresh_token: session.refresh_token.clone(),
        expires_at: session.expires_at,
        saved_at: current_unix_ts(),
    })?;

    if account_created {
        eprintln!("Account created and signed in as {} ({})", session.email, session.user_id);
    } else {
        eprintln!("Signed in as {} ({})", session.email, session.user_id);
    }
    eprintln!("Session saved locally.");

    let set_now = prompt_line_allow_empty("Set password now? [Y/n]: ")?;
    if set_now.trim().is_empty() || matches!(set_now.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        let new_password = prompt_password("New password: ")?;
        let confirm_password = prompt_password("Confirm password: ")?;
        if new_password != confirm_password {
            return Err(anyhow!("password confirmation does not match"));
        }
        auth::set_password(supabase_url, supabase_key, &session.user_id, &session.session_token, &new_password).await?;
        eprintln!("Password set successfully.");
    }

    Ok(())
}

fn run_signup(args: SignupArgs) -> Result<()> {
    ensure_signed_out_for_auth("signup")?;
    let runtime = Runtime::new().context("failed to create async runtime")?;
    runtime.block_on(async move {
        let supabase_url = auth::default_supabase_url()?;
        let supabase_key = auth::default_supabase_anon_key()?;
        eprintln!("\n=== Caesim Signup ===\n");

        let email = match args.email {
            Some(email) => email,
            None => prompt_line("Email address: ")?,
        };

        eprintln!("Sending verification code to {}...", email);
        let login = auth::start_login(&supabase_url, &supabase_key, &email).await?;
        eprintln!("{}", login.message);
        let code = prompt_line("Verification code: ")?;

        complete_otp_session(&supabase_url, &supabase_key, &email, &code, true).await?;
        Ok(())
    })
}

fn run_login(args: LoginArgs) -> Result<()> {
    ensure_signed_out_for_auth("login")?;
    let runtime = Runtime::new().context("failed to create async runtime")?;
    runtime.block_on(async move {
        let supabase_url = auth::default_supabase_url()?;
        let supabase_key = auth::default_supabase_anon_key()?;
        eprintln!("\n=== Caesim Login ===\n");

        let email = match args.email {
            Some(email) => email,
            None => prompt_line("Email address: ")?,
        };

        if args.otp {
            let code = if let Some(code) = args.verification_code {
                code
            } else {
                let login = auth::start_login(&supabase_url, &supabase_key, &email).await?;
                eprintln!("{}", login.message);
                prompt_line("Verification code: ")?
            };

            complete_otp_session(&supabase_url, &supabase_key, &email, &code, false).await?;
            return Ok(());
        }

        let password = prompt_password("Password: ")?;
        match auth::login_with_password(&supabase_url, &supabase_key, &email, &password).await {
            Ok(session) => {
                auth::save_session(&auth::StoredSession {
                    supabase_url: supabase_url.clone(),
                    user_id: session.user_id.clone(),
                    email: session.email.clone(),
                    session_token: session.session_token.clone(),
                    refresh_token: session.refresh_token.clone(),
                    expires_at: session.expires_at,
                    saved_at: current_unix_ts(),
                })?;

                eprintln!("Signed in as {} ({})", session.email, session.user_id);
                eprintln!("Session saved locally.");

                Ok(())
            }
            Err(err) => Err(anyhow!(
                "login failed: {}\nIf this is a new account, run `caesim signup --email <email>`.\nIf you already have an OTP-based account, run `caesim login --otp --email <email>`.",
                err
            )),
        }
    })
}

fn run_whoami() -> Result<()> {
    let runtime = Runtime::new().context("failed to create async runtime")?;
    runtime.block_on(async move {
        let mut session = auth::load_session()?.ok_or_else(|| anyhow!("no local session found; run `caesim login` first"))?;
        let supabase_url = auth::default_supabase_url().unwrap_or_else(|_| session.supabase_url.clone());
        let supabase_key = auth::default_supabase_anon_key()?;
        auth::ensure_session_fresh(&supabase_url, &supabase_key, &mut session).await?;
        let me = auth::fetch_me(&supabase_url, &supabase_key, &session.user_id, &session.session_token).await;

        match me {
            Ok(profile) => {
                eprintln!("Signed in as {}", profile.email);
                eprintln!("User ID: {}", profile.user_id);
                eprintln!("Account status: {}", profile.account_status);
                eprintln!("Credit balance: {} credits", profile.credit_balance);
                eprintln!("Session expires at: {}", profile.expires_at);
                Ok(())
            }
            Err(err) => {
                let message = format!("{}", err);
                if message.contains("401") || message.contains("unauthorized") || message.contains("session expired") {
                    let _ = auth::clear_session();
                }
                Err(err)
            }
        }
    })
}

fn run_logout() -> Result<()> {
    auth::clear_session()?;
    eprintln!("Signed out successfully. Session deleted.");
    Ok(())
}

fn run_change_password(args: ChangePasswordArgs) -> Result<()> {
    let runtime = Runtime::new().context("failed to create async runtime")?;
    runtime.block_on(async move {
        let supabase_url = auth::default_supabase_url()?;
        let supabase_key = auth::default_supabase_anon_key()?;

        if args.otp {
            let email = prompt_line("Email address: ")?;
            let login = auth::start_login(&supabase_url, &supabase_key, &email).await?;
            eprintln!("{}", login.message);
            let code = prompt_line("Verification code: ")?;
            let new_password = prompt_password("New password: ")?;
            let confirm_password = prompt_password("Confirm password: ")?;
            if new_password != confirm_password {
                return Err(anyhow!("password confirmation does not match"));
            }

            auth::change_password_with_otp(&supabase_url, &supabase_key, &email, &code, &new_password).await?;
            eprintln!("Password changed successfully via OTP.");
            return Ok(());
        }

        let mut session = auth::load_session()?.ok_or_else(|| anyhow!("no local session found; run `caesim login` first"))?;
        let supabase_url = auth::default_supabase_url().unwrap_or_else(|_| session.supabase_url.clone());
        let supabase_key = auth::default_supabase_anon_key()?;
        auth::ensure_session_fresh(&supabase_url, &supabase_key, &mut session).await?;

        let profile = auth::fetch_me(&supabase_url, &supabase_key, &session.user_id, &session.session_token).await?;

        if !profile.has_password {
            eprintln!("No password is currently set. Setting a new password...");
            let new_password = prompt_password("New password: ")?;
            let confirm_password = prompt_password("Confirm password: ")?;
            if new_password != confirm_password {
                return Err(anyhow!("password confirmation does not match"));
            }
            auth::set_password(&supabase_url, &supabase_key, &session.user_id, &session.session_token, &new_password).await?;
            eprintln!("Password set successfully.");
            return Ok(());
        }

        let current_password = prompt_password("Current password: ")?;
        let new_password = prompt_password("New password: ")?;
        let confirm_password = prompt_password("Confirm password: ")?;
        if new_password != confirm_password {
            return Err(anyhow!("password confirmation does not match"));
        }

        auth::change_password(
            &supabase_url,
            &supabase_key,
            &session.user_id,
            &session.session_token,
            &current_password,
            &new_password,
        )
        .await?;

        eprintln!("Password changed successfully.");
        Ok(())
    })
}

fn run_credits(args: CreditsArgs) -> Result<()> {
    let runtime = Runtime::new().context("failed to create async runtime")?;
    runtime.block_on(async move {
        let mut session = auth::load_session()?.ok_or_else(|| anyhow!("no local session found; run `caesim login` first"))?;
        if !session.email.trim().eq_ignore_ascii_case("service@trainvent.com") {
            return Err(anyhow!("payment processing is coming soon - contact support to get toy credits for testing"));
        }
        let supabase_url = auth::default_supabase_url().unwrap_or_else(|_| session.supabase_url.clone());
        let supabase_key = auth::default_supabase_anon_key()?;
        auth::ensure_session_fresh(&supabase_url, &supabase_key, &mut session).await?;

        match args.command {
            Some(CreditsCommand::Balance) | None => {
                let me = auth::fetch_me(&supabase_url, &supabase_key, &session.user_id, &session.session_token).await?;
                eprintln!("Credit balance: {} credits", me.credit_balance);
                Ok(())
            }
            Some(CreditsCommand::Add { amount }) => {
                auth::add_credits(&supabase_url, &supabase_key, &session.user_id, &session.session_token, amount).await?;
                eprintln!("Added {} credits.", amount);
                let me = auth::fetch_me(&supabase_url, &supabase_key, &session.user_id, &session.session_token).await?;
                eprintln!("New balance: {} credits", me.credit_balance);
                Ok(())
            }
        }
    })
}

fn prompt_line(prompt: &str) -> Result<String> {
    eprint!("{}", prompt);
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_string();
    if input.is_empty() {
        return Err(anyhow!("no input provided"));
    }
    Ok(input)
}

fn prompt_line_allow_empty(prompt: &str) -> Result<String> {
    eprint!("{}", prompt);
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_password(prompt: &str) -> Result<String> {
    loop {
        eprint!("{}", prompt);
        io::stderr().flush()?;
        let value = match rpassword::read_password() {
            Ok(pwd) => pwd,
            Err(_) => {
                // Fallback to regular input if rpassword fails
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        };
        if !value.trim().is_empty() {
            return Ok(value);
        }
        eprintln!("Password cannot be empty. Please try again.");
    }
}

fn current_unix_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn run_vision(args: VisionArgs) -> Result<()> {
    eprintln!("\n=== Google Cloud Vision Configuration ===\n");

    // Check if GOOGLE_APPLICATION_CREDENTIALS is already set
    if let Ok(creds_path) = env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        eprintln!("✓ GOOGLE_APPLICATION_CREDENTIALS is set to:");
        eprintln!("  {}", creds_path);
        if PathBuf::from(&creds_path).exists() {
            eprintln!("  (file exists)");
        } else {
            eprintln!("  (warning: file not found)");
        }
        eprintln!();
    }

    // Check for default credentials location
    if let Ok(home) = env::var("HOME") {
        let default_creds = PathBuf::from(home).join(".config/gcloud/application_default_credentials.json");
        if default_creds.exists() {
            eprintln!("✓ Default Application Credentials found at:");
            eprintln!("  {}", default_creds.display());
            eprintln!();
        }
    }

    // If custom path provided, set GOOGLE_APPLICATION_CREDENTIALS to it
    if let Some(custom_json) = args.service_account_json {
        if !custom_json.exists() {
            return Err(anyhow!("service account JSON does not exist: {}", custom_json.display()));
        }
        eprintln!("To use custom credentials, set the environment variable:");
        eprintln!("  export GOOGLE_APPLICATION_CREDENTIALS=\"{}\"", custom_json.display());
        eprintln!();
        eprintln!("Then run caesim commands with --find flag.");
        return Ok(());
    }

    // Guide through gcloud auth
    eprintln!("To set up Google Vision credentials, run:");
    eprintln!("  gcloud auth application-default login");
    eprintln!();
    eprintln!("This will:");
    eprintln!("  1. Open a browser to authenticate with your Google account");
    eprintln!("  2. Save credentials to ~/.config/gcloud/application_default_credentials.json");
    eprintln!("  3. Allow caesim to access Google Cloud Vision API");
    eprintln!();
    eprintln!("After setup, you can use caesim with --find flag:");
    eprintln!("  caesim cut <path> --rule duplicates --find cars");
    eprintln!();

    Ok(())
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
    let vision_enabled = args.find.is_some();
    let vision_label = args.find.as_deref();
    let complexity = estimate_complexity(images.len(), vision_enabled);
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
    if vision_label.is_some() && !vision_features.iter().any(|f| f == "LABEL_DETECTION") {
        vision_features.push("LABEL_DETECTION".to_string());
    }

    let mut vision_credit_cost = None;
    let mut credit_balance_before = None;
    let mut credit_balance_after = None;
    let mut vision_credit_runtime = None;
    let mut vision_credit_supabase_url = None;
    let mut vision_credit_supabase_key = None;
    let mut vision_credit_user_id = None;
    let mut vision_credit_session_token = None;

    if vision_enabled {
        let runtime = Runtime::new().context("failed to create async runtime for credit checks")?;
        let mut session = auth::load_session()?.ok_or_else(|| anyhow!("vision mode requires a local session; run `caesim login` first"))?;
        let supabase_url = auth::default_supabase_url().unwrap_or_else(|_| session.supabase_url.clone());
        let supabase_key = auth::default_supabase_anon_key()?;
        runtime.block_on(auth::ensure_session_fresh(&supabase_url, &supabase_key, &mut session))?;
        let me = runtime.block_on(auth::fetch_me(&supabase_url, &supabase_key, &session.user_id, &session.session_token))?;
        let cost = estimate_vision_credit_cost(images.len());

        if me.credit_balance < cost {
            return Err(anyhow!(
                "insufficient credits for vision run: have {}, need {}",
                me.credit_balance,
                cost
            ));
        }

        eprintln!("Toy credits: vision run will cost {} credit(s); current balance is {}.", cost, me.credit_balance);
        vision_credit_cost = Some(cost);
        credit_balance_before = Some(me.credit_balance);
        vision_credit_runtime = Some(runtime);
        vision_credit_supabase_url = Some(supabase_url);
        vision_credit_supabase_key = Some(supabase_key);
        vision_credit_user_id = Some(session.user_id);
        vision_credit_session_token = Some(session.session_token);
    }

    // Optional: call python vision backend to get labels/safesearch/text/web/image properties.
    let vision_map = if vision_enabled {
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

    if let (Some(runtime), Some(supabase_url), Some(supabase_key), Some(user_id), Some(session_token), Some(cost)) = (
        vision_credit_runtime.as_ref(),
        vision_credit_supabase_url.as_ref(),
        vision_credit_supabase_key.as_ref(),
        vision_credit_user_id.as_ref(),
        vision_credit_session_token.as_ref(),
        vision_credit_cost,
    ) {
        let new_balance = runtime.block_on(auth::consume_credits(
            supabase_url,
            supabase_key,
            user_id,
            session_token,
            cost,
        ))?;
        credit_balance_after = Some(new_balance);
        eprintln!("Toy credits: charged {} credit(s); new balance is {}.", cost, new_balance);
    }

    let vision_duplicate_sources = vision_map
        .as_ref()
        .map(|vm| build_vision_duplicate_sources(&images, vm))
        .unwrap_or_default();

    // compute a run id early so the report filename can be prefixed by it
    let run_id = chrono_run_id();

    let report_path = args.report.clone().unwrap_or_else(|| {
        // Default to XDG cache dir if available, otherwise $HOME/.cache, then ./
        let cache_base = std::env::var("XDG_CACHE_HOME").map(PathBuf::from).or_else(|_| {
            std::env::var("HOME").map(|h| PathBuf::from(h).join(".cache"))
        });

        let cache_dir = cache_base.unwrap_or_else(|_| PathBuf::from("."));
        let caesim_cache = cache_dir.join("caesim");
        // Use the target folder name to create a stable report name
        let target_name = target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string());
        caesim_cache.join(format!("{}-{}.caesim-report.json", run_id, target_name))
    });

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
            if normalized_rule == "explicit" && vision_enabled {
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

        if vision_enabled {
            if let Some(query) = vision_label {
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
        run_id: run_id.clone(),
        target_path: target.to_string_lossy().to_string(),
        rule: args.cut_rule.clone(),
        find: args.find.clone(),
        destination: Some(cut_dir_path.to_string_lossy().to_string()),
        dry_run: args.dry_run,
        scanned_count: images.len(),
        matched_count: matched.len(),
        moved_count,
        cut_dir: cut_dir_path.to_string_lossy().to_string(),
        complexity,
        vision_credit_cost,
        credit_balance_before,
        credit_balance_after,
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

fn run_cut_undo(report_path: Option<PathBuf>) -> Result<()> {
    // If the user didn't provide a report path, attempt to find the newest report in cache
    let report_path = match report_path {
        Some(p) => p,
        None => {
            let cache_base = std::env::var("XDG_CACHE_HOME").map(PathBuf::from).or_else(|_| {
                std::env::var("HOME").map(|h| PathBuf::from(h).join(".cache"))
            });

            let cache_dir = cache_base.unwrap_or_else(|_| PathBuf::from("."));
            let caesim_cache = cache_dir.join("caesim");
            if caesim_cache.exists() && caesim_cache.is_dir() {
                let mut latest: Option<(std::time::SystemTime, PathBuf)> = None;
                for entry in fs::read_dir(&caesim_cache).unwrap_or_else(|_| fs::read_dir(".").unwrap()) {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file() && path.to_string_lossy().ends_with(".caesim-report.json") {
                            if let Ok(meta) = entry.metadata() {
                                if let Ok(mtime) = meta.modified() {
                                    match &latest {
                                        Some((t, _)) if *t >= mtime => {}
                                        _ => latest = Some((mtime, path.clone())),
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some((_, p)) = latest {
                    p
                } else {
                    std::env::current_dir()
                        .unwrap_or_else(|_| PathBuf::from("."))
                        .join(".caesim-report.json")
                }
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(".caesim-report.json")
            }
        }
    };

    if !report_path.exists() {
        return Err(anyhow!("report does not exist: {}", report_path.display()));
    }
    if !report_path.is_file() {
        return Err(anyhow!("report is not a file: {}", report_path.display()));
    }

    let report_bytes = fs::read(&report_path)
        .with_context(|| format!("failed to read report {}", report_path.display()))?;
    let report: RunReport = serde_json::from_slice(&report_bytes)
        .with_context(|| format!("failed to parse report {}", report_path.display()))?;

    if report.dry_run {
        eprintln!("Report was a dry run; nothing to restore.");
        return Ok(());
    }

    let mut restored_count = 0usize;
    for entry in &report.entries {
        let Some(destination) = entry.destination.as_ref() else {
            continue;
        };

        let source_path = PathBuf::from(&entry.source);
        let destination_path = PathBuf::from(destination);

        if !destination_path.exists() {
            return Err(anyhow!(
                "cannot restore missing file: {}",
                destination_path.display()
            ));
        }

        if let Some(parent) = source_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let restore_path = unique_restore_destination(&source_path)?;
        fs::rename(&destination_path, &restore_path).with_context(|| {
            format!(
                "failed to restore {} -> {}",
                destination_path.display(),
                restore_path.display()
            )
        })?;
        restored_count += 1;
    }

    if !report.cut_dir.trim().is_empty() {
        let cut_dir_path = PathBuf::from(&report.cut_dir);
        if cut_dir_path.is_dir() && is_dir_empty(&cut_dir_path)? {
            fs::remove_dir(&cut_dir_path).with_context(|| {
                format!("failed to remove empty cut directory {}", cut_dir_path.display())
            })?;
            eprintln!("Removed empty cut directory {}.", cut_dir_path.display());
        }
    }

    eprintln!(
        "Restored {} file(s) from report {}.",
        restored_count,
        report_path.display()
    );
    Ok(())
}

fn is_cut_undo_request(args: &CutArgs) -> bool {
    args.path.as_os_str() == "undo"
        && args.cut_rule.is_none()
        && args.find.is_none()
        && args.destination.is_none()
        && !args.dry_run
        && args.cut_img.is_none()
}

fn is_dir_empty(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("failed to inspect directory {}", path.display()))?;
    Ok(entries.next().is_none())
}

fn run_assist() -> Result<()> {
    let runtime = Runtime::new().context("failed to create async runtime")?;
    runtime.block_on(async {
    eprintln!("\n=== Caesim AI Assistant ===");
    eprintln!("Describe what you'd like to do with your image library.");
    eprintln!("(Type 'help' for examples, or 'quit' to exit)\n");

    let gateway_url = ai_assist::default_gateway_url();
    let api_key = if gateway_url.is_some() {
        None
    } else {
        Some(ai_assist::default_api_key()?)
    };
    let mut session = auth::load_session()?.ok_or_else(|| anyhow!("ai-assist requires a local session; run `caesim login` first"))?;
    let supabase_url = auth::default_supabase_url().unwrap_or_else(|_| session.supabase_url.clone());
    let supabase_key = auth::default_supabase_anon_key()?;
    auth::ensure_session_fresh(&supabase_url, &supabase_key, &mut session).await?;
    let mut thread_id: Option<String> = std::env::var("BACKBOARD_THREAD_ID").ok();

    loop {
        eprint!(">>> ");
        io::stderr().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input == "quit" || input == "exit" {
            eprintln!("Goodbye!");
            break;
        }

        if input == "help" {
            eprintln!("Examples:");
            eprintln!("  'cut landscape photos into a review folder'");
            eprintln!("  'find all duplicate images and move them'");
            eprintln!("  'scan for screenshots and put them in /tmp/screenshots'");
            eprintln!("  'find food images using AI'");
            eprintln!();
            continue;
        }

        if input.is_empty() {
            continue;
        }

        auth::ensure_session_fresh(&supabase_url, &supabase_key, &mut session).await?;
        let me = auth::fetch_me(&supabase_url, &supabase_key, &session.user_id, &session.session_token).await?;
        if me.credit_balance < 1 {
            return Err(anyhow!(
                "insufficient credits for ai-assist: have {}, need 1",
                me.credit_balance
            ));
        }

        eprintln!("Toy credits: ai-assist will cost 1 credit; current balance is {}.", me.credit_balance);
        let new_balance = auth::consume_credits(&supabase_url, &supabase_key, &session.user_id, &session.session_token, 1).await?;
        eprintln!("Toy credits: charged 1 credit; new balance is {}.", new_balance);

        let assist_result = if let Some(gw) = gateway_url.as_deref() {
            ai_assist::interact_via_gateway(gw, &session.session_token, input, thread_id.clone()).await
        } else {
            ai_assist::interact(api_key.as_deref().ok_or_else(|| anyhow!("BACKBOARD_API_KEY_CAESIM or BACKBOARD_API_KEY environment variable not set"))?, input, thread_id.clone()).await
        };

        match assist_result {
            Ok(response) => {
                if let Some(new_thread_id) = response.thread_id.as_ref() {
                    eprintln!("Thread: {}", new_thread_id);
                    thread_id = Some(new_thread_id.clone());
                }
                if let Some(assistant_id) = response.assistant_id.as_ref() {
                    eprintln!("Assistant: {}", assistant_id);
                }
                if let Some(cmd) = &response.command {
                    if let Some(explanation) = &response.explanation {
                        eprintln!("\n{}", explanation);
                    }
                    eprintln!("\nCommand: {}", cmd);
                    eprint!("Execute? (y/n) ");
                    io::stderr().flush()?;

                    let mut confirm = String::new();
                    io::stdin().read_line(&mut confirm)?;
                    if confirm.trim().eq_ignore_ascii_case("y") {
                        eprintln!("\nExecuting: {}", cmd);
                        execute_command(cmd)?;
                    } else {
                        eprintln!("Skipped.");
                    }
                } else if let Some(err) = &response.error {
                    eprintln!("Error: {}", err);
                } else {
                    if let Some(content) = &response.content {
                        eprintln!("Assistant reply: {}", content);
                    }
                    eprintln!("No command generated.");
                }
            }
            Err(e) => eprintln!("Assistant API error: {:#}", e),
        }
        eprintln!();
    }

    Ok(())
    })
}

// `call_backboard` removed; use `ai_assist::interact()` in its place.

fn execute_command(cmd: &str) -> Result<()> {
    let mut shell_cmd = Command::new("sh");
    shell_cmd.arg("-c").arg(cmd);
    let status = shell_cmd.status().context("Failed to execute command")?;

    if !status.success() {
        return Err(anyhow!("Command exited with status: {}", status));
    }
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

fn unique_restore_destination(source: &Path) -> Result<PathBuf> {
    let parent = source
        .parent()
        .ok_or_else(|| anyhow!("missing restore parent: {}", source.display()))?;
    let filename = source
        .file_name()
        .ok_or_else(|| anyhow!("missing filename: {}", source.display()))?;
    let mut dest = parent.join(filename);
    if !dest.exists() {
        return Ok(dest);
    }

    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("");
    for i in 1..10_000u32 {
        let candidate = if ext.is_empty() {
            parent.join(format!("{stem}_{i}"))
        } else {
            parent.join(format!("{stem}_{i}.{ext}"))
        };
        if !candidate.exists() {
            dest = candidate;
            break;
        }
    }

    Ok(dest)
}

fn write_report(path: &Path, report: &RunReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }

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

fn estimate_vision_credit_cost(image_count: usize) -> i64 {
    if image_count == 0 {
        0
    } else {
        image_count as i64
    }
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
    let (width, height) = read_image_dimensions(path)
        .with_context(|| format!("failed to read image dimensions for {}", path.display()))?;
    Ok(orientation_matches_dimensions(width, height, rule))
}

fn read_image_dimensions(path: &Path) -> Result<(u32, u32)> {
    if let Ok(dimensions) = image::image_dimensions(path) {
        return Ok(dimensions);
    }

    let output = Command::new("python3")
        .arg("-c")
        .arg(
            r#"
from PIL import Image
import sys
path = sys.argv[1]
with Image.open(path) as img:
    print(f'{img.width} {img.height}')
"#,
        )
        .arg(path)
        .output()
        .context("python3 fallback failed to start")?;

    if !output.status.success() {
        return Err(anyhow!(
            "python3 fallback failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8(output.stdout).context("python3 fallback returned invalid utf-8")?;
    let mut parts = stdout.split_whitespace();
    let width: u32 = parts
        .next()
        .ok_or_else(|| anyhow!("python3 fallback missing width"))?
        .parse()
        .context("python3 fallback width parse failed")?;
    let height: u32 = parts
        .next()
        .ok_or_else(|| anyhow!("python3 fallback missing height"))?
        .parse()
        .context("python3 fallback height parse failed")?;

    Ok((width, height))
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
        assert_eq!(available_cut_rules(), &["screenshots", "duplicates", "explicit", "landscape", "portrait"]);
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
    fn recognizes_missing_rule_value_error() {
        assert!(is_missing_cut_rule_value_error(
            "error: a value is required for '--rule <CUT_RULE>' but none was supplied"
        ));
        assert!(!is_missing_cut_rule_value_error(
            "error: the following required arguments were not provided: <PATH>"
        ));
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
