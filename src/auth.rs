use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredSession {
    pub backend_base_url: String,
    pub user_id: String,
    pub email: String,
    pub session_token: String,
    pub expires_at: i64,
    pub saved_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyResponse {
    pub user_id: String,
    pub email: String,
    pub session_token: String,
    pub expires_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct PasswordLoginResponse {
    pub user_id: String,
    pub email: String,
    pub session_token: String,
    pub expires_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct MeResponse {
    pub user_id: String,
    pub email: String,
    pub account_status: String,
    pub credit_balance: i64,
    pub has_password: bool,
    pub expires_at: i64,
}

#[derive(Debug, Serialize)]
struct LoginRequest {
    email: String,
}

#[derive(Debug, Serialize)]
struct VerifyRequest {
    email: String,
    verification_code: String,
}

#[derive(Debug, Serialize)]
struct PasswordLoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct SetPasswordRequest {
    password: String,
}

#[derive(Debug, Serialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Serialize)]
struct ChangePasswordOtpRequest {
    email: String,
    verification_code: String,
    new_password: String,
}

pub fn default_backend_base_url() -> String {
    env::var("CAESIM_BACKEND_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string())
}

pub async fn check_user_exists(base_url: &str, email: &str) -> Result<bool> {
    let url = format!("{}/v1/auth/user-exists", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .json(&LoginRequest {
            email: email.trim().to_string(),
        })
        .send()
        .await
        .with_context(|| format!("failed to check user existence at {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("user existence check failed with {}: {}", status, body));
    }

    #[derive(Deserialize)]
    struct ExistsResponse {
        exists: bool,
    }

    let response: ExistsResponse = serde_json::from_str(&body)
        .context("failed to parse existence check response")?;
    Ok(response.exists)
}

pub fn session_path() -> Result<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("caesim/session.json"));
    }

    if let Ok(home) = env::var("HOME") {
        return Ok(PathBuf::from(home).join(".config/caesim/session.json"));
    }

    Err(anyhow!("could not determine session directory; set XDG_CONFIG_HOME or HOME"))
}

pub fn save_session(session: &StoredSession) -> Result<()> {
    let path = session_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create session dir {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(session)?;
    fs::write(&path, json.as_bytes())
        .with_context(|| format!("failed to write session file {}", path.display()))?;
    Ok(())
}

pub fn load_session() -> Result<Option<StoredSession>> {
    let path = session_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read session file {}", path.display()))?;
    let session: StoredSession = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse session file {}", path.display()))?;
    Ok(Some(session))
}

pub fn clear_session() -> Result<()> {
    let path = session_path()?;
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove session file {}", path.display()))?;
    }
    Ok(())
}

fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

pub async fn start_login(base_url: &str, email: &str) -> Result<LoginResponse> {
    let url = format!("{}/v1/auth/login", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .json(&LoginRequest {
            email: email.trim().to_string(),
        })
        .send()
        .await
        .with_context(|| format!("failed to send login request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("login request failed with {}: {}", status, body));
    }

    let response: LoginResponse = serde_json::from_str(&body)
        .context("failed to parse login response")?;
    Ok(response)
}

pub async fn verify_login(base_url: &str, email: &str, verification_code: &str) -> Result<VerifyResponse> {
    let url = format!("{}/v1/auth/verify", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .json(&VerifyRequest {
            email: email.trim().to_string(),
            verification_code: verification_code.trim().to_string(),
        })
        .send()
        .await
        .with_context(|| format!("failed to send verification request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("verification request failed with {}: {}", status, body));
    }

    let response: VerifyResponse = serde_json::from_str(&body)
        .context("failed to parse verify response")?;
    Ok(response)
}

pub async fn login_with_password(base_url: &str, email: &str, password: &str) -> Result<PasswordLoginResponse> {
    let url = format!("{}/v1/auth/login/password", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .json(&PasswordLoginRequest {
            email: email.trim().to_string(),
            password: password.to_string(),
        })
        .send()
        .await
        .with_context(|| format!("failed to send password login request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("password login failed with {}: {}", status, body));
    }

    let response: PasswordLoginResponse = serde_json::from_str(&body)
        .context("failed to parse password login response")?;
    Ok(response)
}

pub async fn set_password(base_url: &str, session_token: &str, new_password: &str) -> Result<()> {
    let url = format!("{}/v1/auth/set-password", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(session_token)
        .json(&SetPasswordRequest {
            password: new_password.to_string(),
        })
        .send()
        .await
        .with_context(|| format!("failed to send set password request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("set password failed with {}: {}", status, body));
    }

    Ok(())
}

pub async fn change_password(base_url: &str, session_token: &str, current_password: &str, new_password: &str) -> Result<()> {
    let url = format!("{}/v1/auth/change-password", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(session_token)
        .json(&ChangePasswordRequest {
            current_password: current_password.to_string(),
            new_password: new_password.to_string(),
        })
        .send()
        .await
        .with_context(|| format!("failed to send change password request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("change password failed with {}: {}", status, body));
    }

    Ok(())
}

pub async fn change_password_with_otp(base_url: &str, email: &str, verification_code: &str, new_password: &str) -> Result<()> {
    let url = format!("{}/v1/auth/change-password/otp", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .json(&ChangePasswordOtpRequest {
            email: email.trim().to_string(),
            verification_code: verification_code.to_string(),
            new_password: new_password.to_string(),
        })
        .send()
        .await
        .with_context(|| format!("failed to send OTP password change request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("OTP password change failed with {}: {}", status, body));
    }

    Ok(())
}

pub async fn fetch_me(base_url: &str, session_token: &str) -> Result<MeResponse> {
    let url = format!("{}/v1/auth/me", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(session_token)
        .send()
        .await
        .with_context(|| format!("failed to send me request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("whoami request failed with {}: {}", status, body));
    }

    let response: MeResponse = serde_json::from_str(&body)
        .context("failed to parse me response")?;
    Ok(response)
}

#[derive(Debug, Serialize)]
struct AddCreditsRequest {
    amount: i64,
}

pub async fn add_credits(base_url: &str, session_token: &str, amount_cents: i64) -> Result<()> {
    let url = format!("{}/v1/credits/add", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(session_token)
        .json(&AddCreditsRequest { amount: amount_cents })
        .send()
        .await
        .with_context(|| format!("failed to send add credits request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("add credits request failed with {}: {}", status, body));
    }

    Ok(())
}
