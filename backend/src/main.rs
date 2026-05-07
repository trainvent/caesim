use anyhow::Context;
use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{env, net::SocketAddr, time::{SystemTime, UNIX_EPOCH}};

#[derive(Clone)]
struct AppState {
    supabase_url: String,
    supabase_key: String,
    http_client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    email: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct VerifyRequest {
    email: String,
    verification_code: String,
}

#[derive(Debug, Deserialize)]
struct PasswordLoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct SetPasswordRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Deserialize)]
struct ChangePasswordOtpRequest {
    email: String,
    verification_code: String,
    new_password: String,
}

#[derive(Debug, Serialize)]
struct VerifyResponse {
    user_id: String,
    email: String,
    session_token: String,
    expires_at: i64,
}

#[derive(Debug, Serialize)]
struct MeResponse {
    user_id: String,
    email: String,
    account_status: String,
    credit_balance: i64,
    has_password: bool,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct AssistantRequest {
    prompt: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssistantResponse {
    command: Option<String>,
    explanation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddCreditsRequest {
    amount: i64,
}

#[derive(Debug, Serialize)]
struct AddCreditsResponse {
    message: String,
}

#[derive(Debug, Serialize)]
struct SuccessResponse {
    message: String,
}

#[derive(Debug, Serialize)]
struct UserExistsResponse {
    exists: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env"));
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt().init();

    let supabase_url = env::var("SUPABASE_URL").context("SUPABASE_URL environment variable not set")?;
    let supabase_key = env::var("SUPABASE_KEY").context("SUPABASE_KEY environment variable not set")?;

    let state = AppState {
        supabase_url,
        supabase_key,
        http_client: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/auth/user-exists", post(user_exists))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/login/password", post(login_password))
        .route("/v1/auth/verify", post(verify))
        .route("/v1/auth/set-password", post(set_password))
        .route("/v1/auth/change-password", post(change_password))
        .route("/v1/auth/change-password/otp", post(change_password_otp))
        .route("/v1/auth/me", get(me))
        .route("/v1/credits/add", post(add_credits))
        .route("/v1/assistant/request", post(assistant_request))
        .with_state(state);

    let port: u16 = env::var("PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "caesim backend listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn user_exists(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<UserExistsResponse>, ApiError> {
    let email = normalize_email(&req.email)?;

    let url = format!("{}/auth/v1/otp", state.supabase_url);
    let body = serde_json::json!({
        "email": email,
        "create_user": false,
    });

    let response = state
        .http_client
        .post(&url)
        .header("apikey", &state.supabase_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to contact Supabase: {e}")))?;

    Ok(Json(UserExistsResponse {
        exists: response.status().is_success(),
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let email = normalize_email(&req.email)?;

    let url = format!("{}/auth/v1/otp", state.supabase_url);
    let body = serde_json::json!({
        "email": email,
        "create_user": true,
        "gotrue_meta_security": {}
    });

    let response = state
        .http_client
        .post(&url)
        .header("apikey", &state.supabase_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to contact Supabase: {e}")))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(ApiError::internal(format!("Supabase error: {error_text}")));
    }

    Ok(Json(LoginResponse {
        email: email.clone(),
        message: format!("Check the email inbox for {email}. Paste the 6-digit OTP code back into caesim."),
    }))
}

async fn verify(
    State(state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    let email = normalize_email(&req.email)?;
    let token = req.verification_code.trim();

    let json = supabase_verify_otp(&state, &email, token).await?;

    let session_token = extract_access_token(&json)?;
    let user = json
        .get("user")
        .cloned()
        .ok_or_else(|| ApiError::internal("Supabase response did not include a user"))?;

    let user_id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::internal("Supabase response did not include a user id"))?
        .to_string();
    let email = user
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| req.email.trim().to_ascii_lowercase());

    let expires_at = unix_ts() + 30 * 24 * 60 * 60;

    Ok(Json(VerifyResponse {
        user_id,
        email,
        session_token,
        expires_at,
    }))
}

async fn login_password(
    State(state): State<AppState>,
    Json(req): Json<PasswordLoginRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    let email = normalize_email(&req.email)?;
    if req.password.trim().is_empty() {
        return Err(ApiError::bad_request("password is required"));
    }

    let url = format!("{}/auth/v1/token?grant_type=password", state.supabase_url);
    let body = serde_json::json!({
        "email": email,
        "password": req.password
    });

    let response = state
        .http_client
        .post(&url)
        .header("apikey", &state.supabase_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to contact Supabase: {e}")))?;

    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::unauthorized(format!("Supabase password login failed: {body_text}")));
    }

    let json: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| ApiError::internal(format!("failed to parse Supabase response: {e}")))?;

    let session_token = extract_access_token(&json)?;
    let user = json
        .get("user")
        .cloned()
        .ok_or_else(|| ApiError::internal("Supabase response did not include a user"))?;

    let user_id = user
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::internal("Supabase response did not include a user id"))?
        .to_string();
    let user_email = user
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| req.email.trim().to_ascii_lowercase());

    let expires_at = unix_ts() + 30 * 24 * 60 * 60;

    Ok(Json(VerifyResponse {
        user_id,
        email: user_email,
        session_token,
        expires_at,
    }))
}

async fn set_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetPasswordRequest>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let token = bearer_token(&headers)?;
    validate_password(&req.password)?;
    update_supabase_password(&state, &token, &req.password).await?;

    Ok(Json(SuccessResponse {
        message: "password updated".to_string(),
    }))
}

async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let token = bearer_token(&headers)?;
    validate_password(&req.new_password)?;

    let user = fetch_supabase_user(&state, &token).await?;

    let verify_url = format!("{}/auth/v1/token?grant_type=password", state.supabase_url);
    let verify_body = serde_json::json!({
        "email": user.email,
        "password": req.current_password
    });

    let verify_response = state
        .http_client
        .post(&verify_url)
        .header("apikey", &state.supabase_key)
        .header("Content-Type", "application/json")
        .json(&verify_body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to verify current password: {e}")))?;

    if !verify_response.status().is_success() {
        let error_text = verify_response.text().await.unwrap_or_default();
        return Err(ApiError::unauthorized(format!("current password verification failed: {error_text}")));
    }

    update_supabase_password(&state, &token, &req.new_password).await?;

    Ok(Json(SuccessResponse {
        message: "password changed".to_string(),
    }))
}

async fn change_password_otp(
    State(state): State<AppState>,
    Json(req): Json<ChangePasswordOtpRequest>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let email = normalize_email(&req.email)?;
    let code = req.verification_code.trim();
    validate_password(&req.new_password)?;

    let json = supabase_verify_otp(&state, &email, code).await?;
    let token = extract_access_token(&json)?;
    update_supabase_password(&state, &token, &req.new_password).await?;

    Ok(Json(SuccessResponse {
        message: "password changed via otp".to_string(),
    }))
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let token = bearer_token(&headers)?;
    let json = fetch_supabase_user_raw(&state, &token).await?;

    let user_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::internal("user has no id"))?
        .to_string();
    let email = json
        .get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::internal("user has no email"))?
        .to_string();

    let has_password = json
        .get("user_metadata")
        .and_then(|m| m.get("has_password"))
        .and_then(|v| v.as_bool())
        .or_else(|| {
            json
                .get("raw_user_meta_data")
                .and_then(|m| m.get("has_password"))
                .and_then(|v| v.as_bool())
        })
        .unwrap_or(false);

    Ok(Json(MeResponse {
        user_id,
        email,
        account_status: "active".to_string(),
        credit_balance: 0,
        has_password,
        expires_at: unix_ts() + 30 * 24 * 60 * 60,
    }))
}

async fn assistant_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AssistantRequest>,
) -> Result<Json<AssistantResponse>, ApiError> {
    let token = bearer_token(&headers)?;
    let user = fetch_supabase_user(&state, &token).await?;
    let (command, explanation) = suggest_command(&req.prompt, req.path.as_deref());

    Ok(Json(AssistantResponse {
        command,
        explanation,
        user_id: Some(user.user_id),
    }))
}

async fn add_credits(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddCreditsRequest>,
) -> Result<Json<AddCreditsResponse>, ApiError> {
    let _token = bearer_token(&headers)?;

    if req.amount <= 0 {
        return Err(ApiError::bad_request("amount must be greater than 0"));
    }

    // TODO: Integrate with payment provider (Stripe, etc.)
    // For now, just acknowledge the request
    // In production, this would process the payment and update user credits in database

    Ok(Json(AddCreditsResponse {
        message: format!("Added {} cents to your account", req.amount),
    }))
}

async fn fetch_supabase_user_raw(state: &AppState, token: &str) -> Result<serde_json::Value, ApiError> {
    let url = format!("{}/auth/v1/user", state.supabase_url);
    let response = state
        .http_client
        .get(&url)
        .header("apikey", &state.supabase_key)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to get user info: {e}")))?;

    if !response.status().is_success() {
        return Err(ApiError::unauthorized("invalid token"));
    }

    response
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("failed to parse user: {e}")))
}

async fn fetch_supabase_user(state: &AppState, token: &str) -> Result<CurrentUser, ApiError> {
    let url = format!("{}/auth/v1/user", state.supabase_url);
    let response = state
        .http_client
        .get(&url)
        .header("apikey", &state.supabase_key)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to get user info: {e}")))?;

    if !response.status().is_success() {
        return Err(ApiError::unauthorized("invalid token"));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("failed to parse user: {e}")))?;

    let user_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::internal("user has no id"))?
        .to_string();
    let email = json
        .get("email")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::internal("user has no email"))?
        .to_string();

    Ok(CurrentUser { user_id, email })
}

async fn supabase_verify_otp(state: &AppState, email: &str, token: &str) -> Result<serde_json::Value, ApiError> {
    let url = format!("{}/auth/v1/verify", state.supabase_url);
    let body = serde_json::json!({
        "email": email,
        "token": token,
        "type": "email"
    });

    let response = state
        .http_client
        .post(&url)
        .header("apikey", &state.supabase_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to contact Supabase: {e}")))?;

    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::unauthorized(format!("Supabase verification failed: {body_text}")));
    }

    serde_json::from_str(&body_text)
        .map_err(|e| ApiError::internal(format!("failed to parse Supabase response: {e}")))
}

fn extract_access_token(json: &serde_json::Value) -> Result<String, ApiError> {
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
        .ok_or_else(|| ApiError::internal("Supabase response did not include an access token"))
}

async fn update_supabase_password(state: &AppState, token: &str, new_password: &str) -> Result<(), ApiError> {
    let url = format!("{}/auth/v1/user", state.supabase_url);
    let body = serde_json::json!({
        "password": new_password,
        "data": {
            "has_password": true
        },
        "user_metadata": {
            "has_password": true
        }
    });

    let response = state
        .http_client
        .put(&url)
        .header("apikey", &state.supabase_key)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to update password: {e}")))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(ApiError::internal(format!("Supabase password update failed: {error_text}")));
    }

    Ok(())
}

fn validate_password(password: &str) -> Result<(), ApiError> {
    if password.trim().len() < 8 {
        return Err(ApiError::bad_request("password must be at least 8 characters"));
    }
    Ok(())
}

#[derive(Debug)]
struct CurrentUser {
    user_id: String,
    email: String,
}

fn normalize_email(email: &str) -> Result<String, ApiError> {
    let trimmed = email.trim().to_ascii_lowercase();
    if trimmed.is_empty() || !trimmed.contains('@') {
        return Err(ApiError::bad_request("email is required"));
    }
    Ok(trimmed)
}

fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let header = headers
        .get(AUTHORIZATION)
        .ok_or_else(|| ApiError::unauthorized("missing authorization header"))?;
    let value = header
        .to_str()
        .map_err(|_| ApiError::unauthorized("invalid authorization header"))?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or_else(|| ApiError::unauthorized("expected Bearer token"))?;
    Ok(token.trim().to_string())
}

fn suggest_command(prompt: &str, path: Option<&str>) -> (Option<String>, String) {
    let prompt_lc = prompt.to_ascii_lowercase();
    let base = path.unwrap_or(".");

    if prompt_lc.contains("landscape") {
        return (
            Some(format!("caesim cut {} --rule landscape --dry-run", base)),
            "Landscape images are best handled with the orientation rule in preview mode first.".to_string(),
        );
    }

    if prompt_lc.contains("portrait") {
        return (
            Some(format!("caesim cut {} --rule portrait --dry-run", base)),
            "Portrait images are best handled with the orientation rule in preview mode first.".to_string(),
        );
    }

    if prompt_lc.contains("screenshot") {
        return (
            Some(format!("caesim cut {} --rule screenshots --dry-run", base)),
            "Screenshots are usually safe to preview before moving.".to_string(),
        );
    }

    if prompt_lc.contains("duplicate") {
        return (
            Some(format!("caesim cut {} --rule duplicates --vision --dry-run", base)),
            "Duplicates benefit from the deterministic and Vision-assisted checks.".to_string(),
        );
    }

    if prompt_lc.contains("food") {
        return (
            Some(format!("caesim cut {} --contains food --vision --dry-run", base)),
            "Food detection should use the Vision label query in preview mode first.".to_string(),
        );
    }

    (
        None,
        "No rule was clear enough to safely turn into a caesim command yet. Ask for screenshots, duplicates, landscape, portrait, or a label like food.".to_string(),
    )
}

fn unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: message.into() }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self { status: StatusCode::UNAUTHORIZED, message: message.into() }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: message.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(ErrorResponse { error: self.message })).into_response()
    }
}
