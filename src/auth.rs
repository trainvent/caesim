use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
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
    env::var("PROJECT_URL").context("PROJECT_URL environment variable not set")
}

pub fn default_supabase_anon_key() -> Result<String> {
    env::var("PUBLISHABLE_KEY").context("PUBLISHABLE_KEY environment variable not set")
}

pub fn default_supabase_service_role_key() -> Option<String> {
    env::var("SERVICE_ROLE_KEY").ok()
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
        // Try to extract error details from Supabase response
        if let Ok(err_json) = serde_json::from_str::<Value>(&body) {
            if let Some(error_desc) = err_json.get("error_description").and_then(|v| v.as_str()) {
                return Err(anyhow!("{}", error_desc));
            }
            if let Some(error_code) = err_json.get("error").and_then(|v| v.as_str()) {
                return Err(anyhow!("Supabase error: {}", error_code));
            }
        }
        return Err(anyhow!("password login failed with {}: {}", status, body));
    }

    let json: Value = serde_json::from_str(&body)
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

pub async fn set_password(base_url: &str, anon_key: &str, user_id: &str, session_token: &str, new_password: &str) -> Result<()> {
    validate_password(new_password)?;
    let mut patch = Map::new();
    patch.insert("has_password".to_string(), Value::Bool(true));
    update_supabase_user(base_url, anon_key, Some(user_id), session_token, Some(new_password), patch).await
}

pub async fn sync_public_user_row(
    base_url: &str,
    user_id: &str,
    email: &str,
    credit_balance: i64,
) -> Result<()> {
    let service_role_key = default_supabase_service_role_key()
        .ok_or_else(|| anyhow!("SERVICE_ROLE_KEY environment variable not set"))?;

    let now = unix_ts();
    let existing = fetch_public_user_row(base_url, &service_role_key, user_id).await?;
    let created_at = existing
        .as_ref()
        .and_then(|row| row.get("created_at"))
        .and_then(|v| v.as_i64())
        .unwrap_or(now);

    let body = serde_json::json!({
        "id": user_id,
        "email": email.trim().to_ascii_lowercase(),
        "auth_provider": existing
            .as_ref()
            .and_then(|row| row.get("auth_provider"))
            .and_then(|v| v.as_str())
            .unwrap_or("email"),
        "plan": existing
            .as_ref()
            .and_then(|row| row.get("plan"))
            .and_then(|v| v.as_str())
            .unwrap_or("beta"),
        "created_at": created_at,
        "last_seen_at": now,
        "pending_code": existing
            .as_ref()
            .and_then(|row| row.get("pending_code"))
            .cloned()
            .unwrap_or(Value::Null),
        "code_expires_at": existing
            .as_ref()
            .and_then(|row| row.get("code_expires_at"))
            .cloned()
            .unwrap_or(Value::Null),
        "account_status": existing
            .as_ref()
            .and_then(|row| row.get("account_status"))
            .and_then(|v| v.as_str())
            .unwrap_or("active"),
        "credit_balance": credit_balance,
    });

    upsert_public_user_row(base_url, &service_role_key, body).await
}

pub async fn change_password(base_url: &str, anon_key: &str, user_id: &str, session_token: &str, current_password: &str, new_password: &str) -> Result<()> {
    validate_password(new_password)?;
    let user = fetch_supabase_user(base_url, anon_key, Some(user_id), session_token).await?;

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

pub async fn fetch_me(base_url: &str, anon_key: &str, user_id: &str, session_token: &str) -> Result<MeResponse> {
    let auth_json = fetch_supabase_user_raw(base_url, anon_key, Some(user_id), session_token).await?;
    let table_row = match fetch_public_user_row_as_authenticated_user(base_url, anon_key, session_token, user_id).await {
        Ok(row) => row,
        Err(_) => {
            if let Some(service_role_key) = default_supabase_service_role_key() {
                fetch_public_user_row(base_url, &service_role_key, user_id).await.ok().flatten()
            } else {
                None
            }
        }
    };

    let user_id = auth_json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("user has no id"))?
        .to_string();
    let email = table_row
        .as_ref()
        .and_then(|row| row.get("email"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| auth_json.get("email").and_then(|v| v.as_str()).map(str::to_string))
        .ok_or_else(|| anyhow!("user has no email"))?;

    let has_password = extract_boolean_metadata(&auth_json, "has_password");
    let credit_balance = table_row
        .as_ref()
        .and_then(|row| row.get("credit_balance"))
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| extract_integer_metadata(&auth_json, "credit_balance").unwrap_or(0));
    let account_status = table_row
        .as_ref()
        .and_then(|row| row.get("account_status"))
        .and_then(|v| v.as_str())
        .unwrap_or("active")
        .to_string();

    // If a credit gateway URL is configured, prefer it for an authoritative balance.
    let credit_gateway = std::env::var("CREDIT_GATEWAY_URL").ok();

    let credit_balance = if let Some(gw) = credit_gateway.as_deref() {
        match gateway_balance(gw, &session_token).await {
            Ok(b) => b,
            Err(_) => credit_balance, // fallback to table or metadata on error
        }
    } else {
        credit_balance
    };

    Ok(MeResponse {
        user_id,
        email,
        account_status,
        credit_balance,
        has_password,
        expires_at: unix_ts() + 30 * 24 * 60 * 60,
    })
}

async fn gateway_balance(gateway_url: &str, session_token: &str) -> Result<i64> {
    let client = Client::new();
    let resp = client
        .post(gateway_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", session_token))
        .json(&serde_json::json!({"action": "balance"}))
        .send()
        .await
        .with_context(|| format!("failed to contact credit gateway at {}", gateway_url))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(anyhow!("credit gateway returned {}: {}", status, body));
    }

    let json: Value = serde_json::from_str(&body).context("failed to parse credit gateway response")?;
    let bal = json
        .get("credit_balance")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("credit gateway did not include credit_balance"))?;
    Ok(bal)
}

async fn gateway_consume(gateway_url: &str, session_token: &str, amount: i64) -> Result<i64> {
    let client = Client::new();
    let resp = client
        .post(gateway_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", session_token))
        .json(&serde_json::json!({"action": "consume", "amount": amount}))
        .send()
        .await
        .with_context(|| format!("failed to contact credit gateway at {}", gateway_url))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if status.as_u16() == 409 {
        return Err(anyhow!("insufficient credits: {}", body));
    }

    if !status.is_success() {
        return Err(anyhow!("credit gateway returned {}: {}", status, body));
    }

    let json: Value = serde_json::from_str(&body).context("failed to parse credit gateway response")?;
    let bal = json
        .get("credit_balance")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("credit gateway did not include credit_balance"))?;
    Ok(bal)
}

async fn gateway_grant(gateway_url: &str, admin_token: &str, user_id: &str, email: &str, amount: i64) -> Result<i64> {
    let client = Client::new();
    let resp = client
        .post(gateway_url)
        .header("Content-Type", "application/json")
        .header("x-caesim-admin-token", admin_token)
        .json(&serde_json::json!({
            "action": "grant",
            "user_id": user_id,
            "email": email,
            "amount": amount,
        }))
        .send()
        .await
        .with_context(|| format!("failed to contact credit gateway at {}", gateway_url))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(anyhow!("credit gateway returned {}: {}", status, body));
    }

    let json: Value = serde_json::from_str(&body).context("failed to parse credit gateway response")?;
    let bal = json
        .get("credit_balance")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("credit gateway did not include credit_balance"))?;
    Ok(bal)
}

async fn gateway_grant_with_bearer(gateway_url: &str, bearer_token: &str, user_id: &str, email: &str, amount: i64) -> Result<i64> {
    let client = Client::new();
    let resp = client
        .post(gateway_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", bearer_token))
        .json(&serde_json::json!({
            "action": "grant",
            "user_id": user_id,
            "email": email,
            "amount": amount,
        }))
        .send()
        .await
        .with_context(|| format!("failed to contact credit gateway at {}", gateway_url))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(anyhow!("credit gateway returned {}: {}", status, body));
    }

    let json: Value = serde_json::from_str(&body).context("failed to parse credit gateway response")?;
    let bal = json
        .get("credit_balance")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("credit gateway did not include credit_balance"))?;
    Ok(bal)
}

pub async fn add_credits(base_url: &str, anon_key: &str, user_id: &str, session_token: &str, amount_credits: i64) -> Result<i64> {
    if amount_credits <= 0 {
        return Err(anyhow!("amount must be greater than 0"));
    }

    let me = fetch_me(base_url, anon_key, user_id, session_token).await?;
    if !is_test_credit_account(&me.email) {
        return Err(anyhow!(
            "credit top-ups are only enabled for basic@trainvent.com during testing"
        ));
    }

    // Prefer the gateway grant path so top-ups are recorded in credit_ledger.
    if let Some(gw) = std::env::var("CREDIT_GATEWAY_URL").ok() {
        // Prefer a configured admin token, but fall back to the signed-in
        // session token if present so a cloud-hosted service account can
        // perform grants without setting secrets locally.
        let new_balance = if let Ok(admin_token) = std::env::var("CREDIT_ADMIN_TOKEN") {
            gateway_grant(&gw, &admin_token, user_id, &me.email, amount_credits).await?
        } else {
            gateway_grant_with_bearer(&gw, session_token, user_id, &me.email, amount_credits).await?
        };
        return Ok(new_balance);
    }

    let current = me.credit_balance;
    let new_balance = current.saturating_add(amount_credits);
    set_credit_balance(base_url, anon_key, user_id, session_token, new_balance).await?;
    Ok(new_balance)
}

pub async fn consume_credits(base_url: &str, anon_key: &str, user_id: &str, session_token: &str, amount_credits: i64) -> Result<i64> {
    if amount_credits <= 0 {
        return Err(anyhow!("amount must be greater than 0"));
    }

    // If a credit gateway is configured, delegate consumption to it.
    if let Some(gw) = std::env::var("CREDIT_GATEWAY_URL").ok() {
        let new_balance = gateway_consume(&gw, session_token, amount_credits).await?;
        return Ok(new_balance);
    }

    let current = fetch_me(base_url, anon_key, user_id, session_token).await?.credit_balance;
    if current < amount_credits {
        return Err(anyhow!("insufficient credits: have {current}, need {amount_credits}"));
    }

    let new_balance = current - amount_credits;
    set_credit_balance(base_url, anon_key, user_id, session_token, new_balance).await?;
    Ok(new_balance)
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

async fn fetch_supabase_user_raw(base_url: &str, anon_key: &str, user_id: Option<&str>, session_token: &str) -> Result<Value> {
    if let (Some(service_role_key), Some(user_id)) = (default_supabase_service_role_key(), user_id) {
        let url = format!("{}/auth/v1/admin/users/{}", normalize_base_url(base_url), user_id);
        let response = Client::new()
            .get(&url)
            .header("apikey", &service_role_key)
            .header("Authorization", format!("Bearer {service_role_key}"))
            .send()
            .await
            .with_context(|| format!("failed to get admin user info at {url}"))?;

        if response.status().is_success() {
            return response
                .json()
                .await
                .context("failed to parse Supabase admin user response");
        }
    }

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

async fn fetch_supabase_user(base_url: &str, anon_key: &str, user_id: Option<&str>, session_token: &str) -> Result<CurrentUser> {
    let json = fetch_supabase_user_raw(base_url, anon_key, user_id, session_token).await?;
    let email = json
        .get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("user has no email"))?
        .to_string();

    Ok(CurrentUser { email })
}

async fn supabase_verify_otp(base_url: &str, anon_key: &str, email: &str, token: &str) -> Result<Value> {
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
        return Err(anyhow!("Supabase verification failed: {}", body_text));
    }

    serde_json::from_str(&body_text).context("failed to parse Supabase verification response")
}

fn extract_access_token(json: &Value) -> Result<String> {
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

async fn set_credit_balance(base_url: &str, anon_key: &str, user_id: &str, session_token: &str, credit_balance: i64) -> Result<()> {
    if default_supabase_service_role_key().is_some() {
        let auth_json = fetch_supabase_user_raw(base_url, anon_key, Some(user_id), session_token).await?;
        let email = auth_json
            .get("email")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("user has no email"))?;
        sync_public_user_row(base_url, user_id, email, credit_balance).await
    } else {
        let mut patch = Map::new();
        patch.insert("credit_balance".to_string(), Value::from(credit_balance));
        update_supabase_user(base_url, anon_key, Some(user_id), session_token, None, patch).await
    }
}

async fn update_supabase_password(base_url: &str, anon_key: &str, session_token: &str, new_password: &str) -> Result<()> {
    let mut patch = Map::new();
    patch.insert("has_password".to_string(), Value::Bool(true));
    update_supabase_user(base_url, anon_key, None, session_token, Some(new_password), patch).await
}

async fn update_supabase_user(base_url: &str, anon_key: &str, user_id: Option<&str>, session_token: &str, new_password: Option<&str>, metadata_patch: Map<String, Value>) -> Result<()> {
    let mut metadata = fetch_supabase_user_metadata(base_url, anon_key, user_id, session_token).await?;
    for (key, value) in metadata_patch {
        metadata.insert(key, value);
    }

    let mut body = Map::new();
    if let Some(new_password) = new_password {
        body.insert("password".to_string(), Value::String(new_password.to_string()));
    }
    body.insert("data".to_string(), Value::Object(metadata.clone()));
    body.insert("user_metadata".to_string(), Value::Object(metadata.clone()));

    let client = Client::new();
    let response = if let (Some(service_role_key), Some(user_id)) = (default_supabase_service_role_key(), user_id) {
        let url = format!("{}/auth/v1/admin/users/{}", normalize_base_url(base_url), user_id);
        client
            .put(&url)
            .header("apikey", &service_role_key)
            .header("Authorization", format!("Bearer {service_role_key}"))
            .header("Content-Type", "application/json")
            .json(&Value::Object(body))
            .send()
            .await
            .with_context(|| format!("failed to update admin user profile at {url}"))?
    } else {
        let url = format!("{}/auth/v1/user", normalize_base_url(base_url));
        client
            .put(&url)
            .header("apikey", anon_key)
            .header("Authorization", format!("Bearer {session_token}"))
            .header("Content-Type", "application/json")
            .json(&Value::Object(body))
            .send()
            .await
            .with_context(|| format!("failed to update user profile at {url}"))?
    };

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Supabase profile update failed: {}", error_text));
    }

    Ok(())
}

async fn fetch_public_user_row(base_url: &str, service_role_key: &str, user_id: &str) -> Result<Option<Value>> {
    let url = format!("{}/rest/v1/users?select=*&id=eq.{}&limit=1", normalize_base_url(base_url), user_id);
    let response = Client::new()
        .get(&url)
        .header("apikey", service_role_key)
        .header("Authorization", format!("Bearer {service_role_key}"))
        .send()
        .await
        .with_context(|| format!("failed to fetch public user row at {url}"))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(anyhow!("failed to fetch public user row: {}", error_text));
    }

    let rows: Vec<Value> = response
        .json()
        .await
        .context("failed to parse public user row response")?;

    Ok(rows.into_iter().next())
}

async fn fetch_public_user_row_as_authenticated_user(
    base_url: &str,
    anon_key: &str,
    session_token: &str,
    user_id: &str,
) -> Result<Option<Value>> {
    let url = format!("{}/rest/v1/users?select=*&id=eq.{}&limit=1", normalize_base_url(base_url), user_id);
    let response = Client::new()
        .get(&url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .with_context(|| format!("failed to fetch public user row at {url}"))?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let rows: Vec<Value> = response
        .json()
        .await
        .context("failed to parse public user row response")?;

    Ok(rows.into_iter().next())
}

async fn upsert_public_user_row(base_url: &str, service_role_key: &str, row: Value) -> Result<()> {
    let url = format!("{}/rest/v1/users?on_conflict=id", normalize_base_url(base_url));
    let response = Client::new()
        .post(&url)
        .header("apikey", service_role_key)
        .header("Authorization", format!("Bearer {service_role_key}"))
        .header("Content-Type", "application/json")
        .header("Prefer", "resolution=merge-duplicates,return=representation")
        .json(&row)
        .send()
        .await
        .with_context(|| format!("failed to upsert public user row at {url}"))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(anyhow!("failed to upsert public user row: {}", error_text));
    }

    Ok(())
}

async fn fetch_supabase_user_metadata(base_url: &str, anon_key: &str, user_id: Option<&str>, session_token: &str) -> Result<Map<String, Value>> {
    let json = fetch_supabase_user_raw(base_url, anon_key, user_id, session_token).await?;
    Ok(json
        .get("user_metadata")
        .or_else(|| json.get("raw_user_meta_data"))
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default())
}

fn extract_boolean_metadata(user_json: &Value, key: &str) -> bool {
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

fn extract_integer_metadata(user_json: &Value, key: &str) -> Option<i64> {
    ["user_metadata", "raw_user_meta_data"]
        .into_iter()
        .find_map(|field| {
            user_json
                .get(field)
                .and_then(|meta| meta.get(key))
                .and_then(|value| value.as_i64())
        })
}

fn is_test_credit_account(email: &str) -> bool {
    email.trim().eq_ignore_ascii_case("service@trainvent.com")
}

#[derive(Debug)]
struct CurrentUser {
    email: String,
}
