use anyhow::Context;
use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use hex::encode as hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{postgres::{PgConnectOptions, PgPoolOptions}, FromRow, PgPool};
use std::{env, net::SocketAddr, process::Command, time::{SystemTime, UNIX_EPOCH}};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
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
    user_id: String,
    email: String,
    verification_code: String,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct VerifyRequest {
    email: String,
    verification_code: String,
}

#[derive(Debug, Serialize)]
struct VerifyResponse {
    user_id: String,
    email: String,
    session_token: String,
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

#[derive(Debug, FromRow)]
struct UserRow {
    id: String,
    email: String,
    pending_code: Option<String>,
    code_expires_at: Option<i64>,
}

#[derive(Debug, FromRow)]
struct SessionRow {
    user_id: String,
    expires_at: i64,
    revoked_at: Option<i64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env"));
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .init();

    let database_url = resolve_database_url()?;
    let connect_options = database_url
        .parse::<PgConnectOptions>()?
        .statement_cache_capacity(0);
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options)
        .await
        .with_context(|| format!("failed to connect to database at {database_url}"))?;

    init_db(&pool).await?;

    let state = AppState { pool };
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/verify", post(verify))
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
    let now = unix_ts();
    let expires_at = now + 10 * 60;
    let verification_code = Uuid::new_v4().simple().to_string()[..8].to_ascii_uppercase();
    let user_id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO users (id, email, auth_provider, plan, created_at, last_seen_at, pending_code, code_expires_at)
        VALUES ($1, $2, 'email', 'beta', $3, $3, $4, $5)
        ON CONFLICT(email) DO UPDATE SET
            last_seen_at = excluded.last_seen_at,
            pending_code = excluded.pending_code,
            code_expires_at = excluded.code_expires_at
        "#,
    )
    .persistent(false)
    .bind(&user_id)
    .bind(&email)
    .bind(now)
    .bind(&verification_code)
    .bind(expires_at)
    .execute(&state.pool)
    .await
    .map_err(ApiError::db)?;

    let user = fetch_user_by_email(&state.pool, &email).await?.ok_or_else(|| ApiError::internal("user was not created"))?;

    Ok(Json(LoginResponse {
        user_id: user.id,
        email: user.email,
        verification_code,
        expires_at,
    }))
}

async fn verify(
    State(state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    let email = normalize_email(&req.email)?;
    let now = unix_ts();
    let user = fetch_user_by_email(&state.pool, &email)
        .await?
        .ok_or_else(|| ApiError::unauthorized("unknown email"))?;

    let pending_code = user
        .pending_code
        .ok_or_else(|| ApiError::unauthorized("no pending verification code"))?;
    let code_expires_at = user
        .code_expires_at
        .ok_or_else(|| ApiError::unauthorized("verification expired"))?;

    if code_expires_at < now {
        return Err(ApiError::unauthorized("verification code expired"));
    }

    if pending_code != req.verification_code.trim().to_ascii_uppercase() {
        return Err(ApiError::unauthorized("invalid verification code"));
    }

    let session_token = Uuid::new_v4().to_string();
    let token_hash = hash_token(&session_token);
    let expires_at = now + 30 * 24 * 60 * 60;
    let session_id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, session_token_hash, expires_at, created_at, revoked_at)
        VALUES ($1, $2, $3, $4, $5, NULL)
        "#,
    )
    .persistent(false)
    .bind(&session_id)
    .bind(&user.id)
    .bind(&token_hash)
    .bind(expires_at)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(ApiError::db)?;

    sqlx::query("UPDATE users SET pending_code = NULL, code_expires_at = NULL, last_seen_at = $2 WHERE id = $1")
        .persistent(false)
        .bind(&user.id)
        .bind(now)
        .execute(&state.pool)
        .await
        .map_err(ApiError::db)?;

    Ok(Json(VerifyResponse {
        user_id: user.id,
        email: user.email,
        session_token,
        expires_at,
    }))
}

async fn assistant_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AssistantRequest>,
) -> Result<Json<AssistantResponse>, ApiError> {
    let token = bearer_token(&headers)?;
    let session = find_session_by_token(&state.pool, &token)
        .await?
        .ok_or_else(|| ApiError::unauthorized("invalid session token"))?;

    if session.revoked_at.is_some() || session.expires_at < unix_ts() {
        return Err(ApiError::unauthorized("session expired"));
    }

    let user_id = session.user_id;
    let (command, explanation) = suggest_command(&req.prompt, req.path.as_deref());

    let now = unix_ts();
    let assistant_run_id = Uuid::new_v4().to_string();
    let usage_event_id = Uuid::new_v4().to_string();

    sqlx::query(
        r#"
        INSERT INTO assistant_runs (id, user_id, request_text, suggested_command, accepted, executed, exit_code, created_at)
        VALUES ($1, $2, $3, $4, FALSE, FALSE, NULL, $5)
        "#,
    )
    .persistent(false)
    .bind(&assistant_run_id)
    .bind(&user_id)
    .bind(&req.prompt)
    .bind(&command)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(ApiError::db)?;

    sqlx::query(
        r#"
        INSERT INTO usage_events (id, user_id, event_type, image_count, request_tokens, response_tokens, estimated_cost, created_at)
        VALUES ($1, $2, 'assistant_request', 0, 0, 0, 0.0, $3)
        "#,
    )
    .persistent(false)
    .bind(&usage_event_id)
    .bind(&user_id)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(ApiError::db)?;

    Ok(Json(AssistantResponse {
        command,
        explanation,
        user_id: Some(user_id),
    }))
}

async fn init_db(pool: &PgPool) -> anyhow::Result<()> {
    for statement in [
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            email TEXT NOT NULL UNIQUE,
            auth_provider TEXT NOT NULL,
            plan TEXT NOT NULL,
            created_at BIGINT NOT NULL,
            last_seen_at BIGINT NOT NULL,
            pending_code TEXT,
            code_expires_at BIGINT
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            session_token_hash TEXT NOT NULL UNIQUE,
            expires_at BIGINT NOT NULL,
            created_at BIGINT NOT NULL,
            revoked_at BIGINT,
            FOREIGN KEY(user_id) REFERENCES users(id)
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS usage_events (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            image_count INTEGER NOT NULL DEFAULT 0,
            request_tokens INTEGER NOT NULL DEFAULT 0,
            response_tokens INTEGER NOT NULL DEFAULT 0,
            estimated_cost DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            created_at BIGINT NOT NULL,
            FOREIGN KEY(user_id) REFERENCES users(id)
        )
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS assistant_runs (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            request_text TEXT NOT NULL,
            suggested_command TEXT,
            accepted BOOLEAN NOT NULL,
            executed BOOLEAN NOT NULL,
            exit_code INTEGER,
            created_at BIGINT NOT NULL,
            FOREIGN KEY(user_id) REFERENCES users(id)
        )
        "#,
    ] {
        sqlx::query(statement).persistent(false).execute(pool).await?;
    }

    Ok(())
}

async fn fetch_user_by_email(pool: &PgPool, email: &str) -> Result<Option<UserRow>, ApiError> {
    let user = sqlx::query_as::<_, UserRow>(
        r#"
        SELECT id, email, pending_code, code_expires_at
        FROM users
        WHERE email = $1
        "#,
    )
    .persistent(false)
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::db)?;

    Ok(user)
}

async fn find_session_by_token(pool: &PgPool, token: &str) -> Result<Option<SessionRow>, ApiError> {
    let token_hash = hash_token(token);
    let session = sqlx::query_as::<_, SessionRow>(
        r#"
        SELECT user_id, expires_at, revoked_at
        FROM sessions
        WHERE session_token_hash = $1
        "#,
    )
    .persistent(false)
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::db)?;

    Ok(session)
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

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex_encode(digest)
}

fn unix_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn resolve_database_url() -> anyhow::Result<String> {
    if let Ok(url) = env::var("DATABASE_URL") {
        return Ok(url);
    }

    if let Some(url) = read_supabase_url_from_keychain("Ceasim_Supabase")? {
        return Ok(url);
    }

    Ok("postgres://postgres:postgres@localhost:5432/caesim".to_string())
}

fn read_supabase_url_from_keychain(entry_name: &str) -> anyhow::Result<Option<String>> {
    let lookup_args = [
        ["lookup", "label", entry_name],
        ["lookup", "service", entry_name],
        ["lookup", "name", entry_name],
        ["lookup", "username", entry_name],
    ];

    for args in lookup_args {
        let output = Command::new("secret-tool").args(args).output();
        let Ok(output) = output else { continue };
        if !output.status.success() {
            continue;
        }
        let value = String::from_utf8(output.stdout).context("keychain secret is not valid utf-8")?;
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }

    Ok(None)
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

    fn db(err: sqlx::Error) -> Self {
        Self::internal(format!("database error: {err}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(ErrorResponse { error: self.message })).into_response()
    }
}
