use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredSession {
    #[serde(alias = "backend_base_url")]
    pub supabase_url: String,
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

#[derive(Debug, Serialize)]
struct PasswordLoginRequest {
    email: String,
    password: String,
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

pub fn default_supabase_url() -> Result<String> {
    env::var("CAESIM_SUPABASE_URL")
        .or_else(|_| env::var("SUPABASE_URL"))
        .context("CAESIM_SUPABASE_URL or SUPABASE_URL environment variable not set")
}

pub fn default_supabase_anon_key() -> Result<String> {
    env::var("CAESIM_SUPABASE_ANON_KEY")
        .or_else(|_| env::var("SUPABASE_ANON_KEY"))
        .or_else(|_| env::var("SUPABASE_KEY"))
        .context("CAESIM_SUPABASE_ANON_KEY, SUPABASE_ANON_KEY, or SUPABASE_KEY environment variable not set")
}

pub async fn check_user_exists(base_url: &str, anon_key: &str, email: &str) -> Result<bool> {
    let url = format!("{}/auth/v1/otp", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .header("apikey", anon_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "email": email.trim().to_string(),
            "create_user": false,
        }))
        .send()
        .await
        .with_context(|| format!("failed to check user existence at {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("user existence check failed with {}: {}", status, body));
    }

    Ok(true)
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

pub async fn start_login(base_url: &str, anon_key: &str, email: &str) -> Result<LoginResponse> {
    let url = format!("{}/auth/v1/otp", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .header("apikey", anon_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "email": email.trim().to_string(),
            "create_user": true,
            "gotrue_meta_security": {}
        }))
        .send()
        .await
        .with_context(|| format!("failed to send login request to {url}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("login request failed with {}: {}", status, body));
    }

    let email = email.trim().to_string();
    Ok(LoginResponse {
        message: format!("Check the email inbox for {email}. Paste the 6-digit OTP code back into caesim."),
    })
}

pub async fn verify_login(base_url: &str, anon_key: &str, email: &str, verification_code: &str) -> Result<VerifyResponse> {
    let json = supabase_verify_otp(base_url, anon_key, email, verification_code).await?;
    let session_token = extract_access_token(&json)?;
    let user = json
        .get("user")
        .cloned()
        .ok_or_else(|| anyhow!("Supabase response did not include a user"))?;

    let user_id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Supabase response did not include a user id"))?
        .to_string();
    let email = user
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| email.trim().to_ascii_lowercase());

    let expires_at = unix_ts() + 30 * 24 * 60 * 60;

    Ok(VerifyResponse {
        user_id,
        email,
        session_token,
        expires_at,
    })
}

pub async fn login_with_password(base_url: &str, anon_key: &str, email: &str, password: &str) -> Result<PasswordLoginResponse> {
    let url = format!("{}/auth/v1/token?grant_type=password", normalize_base_url(base_url));
    let client = Client::new();
    let resp = client
        .post(&url)
        .header("apikey", anon_key)
        .header("Content-Type", "application/json")
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

    let json: serde_json::Value = serde_json::from_str(&body)
        .context("failed to parse password login response")?;
    let session_token = extract_access_token(&json)?;
    let user = json
        .get("user")
        .cloned()
        .ok_or_else(|| anyhow!("Supabase response did not include a user"))?;

    let user_id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Supabase response did not include a user id"))?
        .to_string();
    let user_email = user
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| email.trim().to_ascii_lowercase());

    let expires_at = unix_ts() + 30 * 24 * 60 * 60;

    Ok(PasswordLoginResponse {
        user_id,
        email: user_email,
        session_token,
        expires_at,
    })
}

pub async fn set_password(base_url: &str, anon_key: &str, session_token: &str, new_password: &str) -> Result<()> {
    validate_password(new_password)?;
    update_supabase_password(base_url, anon_key, session_token, new_password).await
}

pub async fn change_password(base_url: &str, anon_key: &str, session_token: &str, current_password: &str, new_password: &str) -> Result<()> {
    validate_password(new_password)?;
    let user = fetch_supabase_user(base_url, anon_key, session_token).await?;

    let verify_url = format!("{}/auth/v1/token?grant_type=password", normalize_base_url(base_url));
    let verify_body = serde_json::json!({
        "email": user.email,
        "password": current_password
    });

    let verify_response = Client::new()
        .post(&verify_url)
        .header("apikey", anon_key)
        .header("Content-Type", "application/json")
        .json(&verify_body)
        .send()
        .await
        .with_context(|| format!("failed to verify current password at {verify_url}"))?;

    if !verify_response.status().is_success() {
        let error_text = verify_response.text().await.unwrap_or_default();
        return Err(anyhow!("current password verification failed: {}", error_text));
    }

    update_supabase_password(base_url, anon_key, session_token, new_password).await
}

pub async fn change_password_with_otp(base_url: &str, anon_key: &str, email: &str, verification_code: &str, new_password: &str) -> Result<()> {
    validate_password(new_password)?;
    let json = supabase_verify_otp(base_url, anon_key, email, verification_code).await?;
    let token = extract_access_token(&json)?;
    update_supabase_password(base_url, anon_key, &token, new_password).await
}

pub async fn fetch_me(base_url: &str, anon_key: &str, session_token: &str) -> Result<MeResponse> {
    let json = fetch_supabase_user_raw(base_url, anon_key, session_token).await?;

    let user_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("user has no id"))?
        .to_string();
    let email = json
        .get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("user has no email"))?
        .to_string();

    let has_password = extract_boolean_metadata(&json, "has_password");
    let credit_balance = extract_integer_metadata(&json, "credit_balance").unwrap_or(0);

    Ok(MeResponse {
        user_id,
        email,
        account_status: "active".to_string(),
        credit_balance,
        has_password,
        expires_at: unix_ts() + 30 * 24 * 60 * 60,
    })
}

pub async fn add_credits(_base_url: &str, _session_token: &str, _amount_cents: i64) -> Result<()> {
    Err(anyhow!("credit top-ups are not available without a hosted billing service"))
}

fn unix_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn validate_password(password: &str) -> Result<()> {
    if password.trim().len() < 8 {
        return Err(anyhow!("password must be at least 8 characters"));
    }
    Ok(())
}

async fn fetch_supabase_user_raw(base_url: &str, anon_key: &str, session_token: &str) -> Result<serde_json::Value> {
    let url = format!("{}/auth/v1/user", normalize_base_url(base_url));
    let response = Client::new()
        .get(&url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .with_context(|| format!("failed to get user info at {url}"))?;

    if !response.status().is_success() {
        return Err(anyhow!("invalid token"));
    }

    response
        .json()
        .await
        .context("failed to parse Supabase user response")
}

async fn fetch_supabase_user(base_url: &str, anon_key: &str, session_token: &str) -> Result<CurrentUser> {
    let json = fetch_supabase_user_raw(base_url, anon_key, session_token).await?;
    let email = json
        .get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("user has no email"))?
        .to_string();

    Ok(CurrentUser { email })
}

async fn supabase_verify_otp(base_url: &str, anon_key: &str, email: &str, token: &str) -> Result<serde_json::Value> {
    let url = format!("{}/auth/v1/verify", normalize_base_url(base_url));
    let body = serde_json::json!({
        "email": email.trim().to_ascii_lowercase(),
        "token": token.trim(),
        "type": "email"
    });

    let response = Client::new()
        .post(&url)
        .header("apikey", anon_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("failed to contact Supabase at {url}"))?;

    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("Supabase verification failed with {}: {}", status, body_text));
    }

    serde_json::from_str(&body_text)
        .context("failed to parse Supabase verification response")
}

fn extract_access_token(json: &serde_json::Value) -> Result<String> {
    json
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            json.get("session")
                .and_then(|s| s.get("access_token"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow!("Supabase response did not include an access token"))
}

async fn update_supabase_password(base_url: &str, anon_key: &str, session_token: &str, new_password: &str) -> Result<()> {
    let url = format!("{}/auth/v1/user", normalize_base_url(base_url));
    let body = serde_json::json!({
        "password": new_password,
        "user_metadata": {
            "has_password": true
        }
    });

    let response = Client::new()
        .put(&url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {session_token}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("failed to update password at {url}"))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Supabase password update failed: {}", error_text));
    }

    Ok(())
}

fn extract_boolean_metadata(user_json: &serde_json::Value, key: &str) -> bool {
    ["user_metadata", "raw_user_meta_data"]
        .into_iter()
        .find_map(|field| {
            user_json
                .get(field)
                .and_then(|meta| meta.get(key))
                .and_then(|value| value.as_bool())
        })
        .unwrap_or(false)
}

fn extract_integer_metadata(user_json: &serde_json::Value, key: &str) -> Option<i64> {
    ["user_metadata", "raw_user_meta_data"]
        .into_iter()
        .find_map(|field| {
            user_json
                .get(field)
                .and_then(|meta| meta.get(key))
                .and_then(|value| value.as_i64())
        })
}

#[derive(Debug)]
struct CurrentUser {
    email: String,
}
