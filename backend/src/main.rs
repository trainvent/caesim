use anyhow::Context;
use axum::{
    extract::State,
    http::HeaderMap,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{env, net::SocketAddr, time::{SystemTime, UNIX_EPOCH}};
use uuid::Uuid;

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

#[derive(Debug, Serialize, Deserialize)]
struct SupabaseUser {
    id: String,
    email: Option<String>,
    user_metadata: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    user_id: String,
    email: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct VerifyRequest {
    access_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserProfile {
    id: String,
    email: String,
    account_status: String,
    credit_balance: i64,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env"));
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .init();

    let supabase_url = env::var("SUPABASE_URL")
        .context("SUPABASE_URL environment variable not set")?;
    let supabase_key = env::var("SUPABASE_KEY")
        .context("SUPABASE_KEY environment variable not set")?;

    let state = AppState {
        supabase_url,
        supabase_key,
        http_client: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/verify", post(verify))
        .route("/v1/auth/me", get(me))
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

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let email = normalize_email(&req.email)?;

    // Send magic link via Supabase Auth
    let url = format!("{}/auth/v1/otp", state.supabase_url);
    let body = serde_json::json!({
        "email": email,
        "create_user": true
    });

    let response = state
        .http_client
        .post(&url)
        .header("apikey", &state.supabase_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to contact Supabase: {e}")))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(ApiError::internal(format!("Supabase error: {error_text}")));
    }

    // Ensure user profile exists in public schema
    ensure_user_profile(&state, &email).await?;

    Ok(Json(LoginResponse {
        user_id: "pending".to_string(),
        email: email.clone(),
        message: format!("Magic link sent to {email}. Check your email to complete login."),
    }))
}

async fn verify(
    State(state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    // Get user info from Supabase using the access token
    let url = format!("{}/auth/v1/user", state.supabase_url);
    let response = state
        .http_client
        .get(&url)
        .header("apikey", &state.supabase_key)
        .header("Authorization", format!("Bearer {}", req.access_token))
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to get user info: {e}")))?;

    if !response.status().is_success() {
        return Err(ApiError::unauthorized("invalid access token"));
    }

    let user: SupabaseUser = response
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("failed to parse user info: {e}")))?;

    let email = user.email.clone().ok_or_else(|| ApiError::internal("user has no email"))?;

    // Ensure user profile exists
    ensure_user_profile(&state, &email).await?;

    let session_token = Uuid::new_v4().to_string();
    let expires_at = unix_ts() + 30 * 24 * 60 * 60;

    Ok(Json(VerifyResponse {
        user_id: user.id,
        email,
        session_token,
        expires_at,
    }))
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let token = bearer_token(&headers)?;

    // Validate token with Supabase
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

    let user: SupabaseUser = response
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("failed to parse user: {e}")))?;

    let email = user.email.ok_or_else(|| ApiError::internal("user has no email"))?;

    // Fetch user profile from Supabase
    let profile = fetch_user_profile(&state, &user.id).await?;

    Ok(Json(MeResponse {
        user_id: user.id,
        email,
        account_status: profile.account_status,
        credit_balance: profile.credit_balance,
        expires_at: unix_ts() + 30 * 24 * 60 * 60,
    }))
}

async fn assistant_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AssistantRequest>,
) -> Result<Json<AssistantResponse>, ApiError> {
    let token = bearer_token(&headers)?;

    // Validate token with Supabase
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

    let user: SupabaseUser = response
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("failed to parse user: {e}")))?;

    let (command, explanation) = suggest_command(&req.prompt, req.path.as_deref());

    Ok(Json(AssistantResponse {
        command,
        explanation,
        user_id: Some(user.id),
    }))
}

async fn ensure_user_profile(state: &AppState, email: &str) -> Result<(), ApiError> {
    let url = format!("{}/rest/v1/user_profiles", state.supabase_url);
    let body = serde_json::json!({
        "email": email,
        "account_status": "active",
        "credit_balance": 0
    });

    let _response = state
        .http_client
        .post(&url)
        .header("apikey", &state.supabase_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .ok();

    Ok(())
}

async fn fetch_user_profile(state: &AppState, user_id: &str) -> Result<UserProfile, ApiError> {
    let url = format!("{}/rest/v1/user_profiles?id=eq.{}", state.supabase_url, user_id);
    let response = state
        .http_client
        .get(&url)
        .header("apikey", &state.supabase_key)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("failed to fetch profile: {e}")))?;

    if !response.status().is_success() {
        return Ok(UserProfile {
            id: user_id.to_string(),
            email: "unknown".to_string(),
            account_status: "active".to_string(),
            credit_balance: 0,
        });
    }

    let profiles: Vec<UserProfile> = response
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("failed to parse profile: {e}")))?;

    Ok(profiles.into_iter().next().unwrap_or(UserProfile {
        id: user_id.to_string(),
        email: "unknown".to_string(),
        account_status: "active".to_string(),
        credit_balance: 0,
    }))
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
        .get("authorization")
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
