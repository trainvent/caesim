use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use reqwest::Client;

mod config;
pub use config::AssistantConfig;

#[derive(Serialize, Deserialize, Debug)]
struct AssistRequest {
    content: String,
    assistant_id: Option<String>,
    thread_id: Option<String>,
    system_prompt: Option<String>,
    json_output: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AssistResponse {
    pub thread_id: Option<String>,
    pub assistant_id: Option<String>,
    pub content: Option<String>,
    pub status: Option<String>,
    pub command: Option<String>,
    pub explanation: Option<String>,
    pub error: Option<String>,
    #[serde(flatten)]
    pub raw: Value,
}

pub async fn interact(api_key: &str, user_text: &str, thread_id: Option<String>) -> Result<AssistResponse> {
    let cfg = AssistantConfig::load_from_env()?;

    let req = AssistRequest {
        content: user_text.to_string(),
        assistant_id: cfg.assistant_id.clone(),
        thread_id: thread_id.or(cfg.thread_id.clone()),
        system_prompt: cfg.system_prompt.clone(),
        json_output: true,
    };

    let client = Client::new();
    let base = cfg.api_base.trim_end_matches('/');
    let url = format!("{}/threads/messages", base);

    let resp = client
        .post(&url)
        .header("X-API-Key", api_key)
        .header("Accept", "application/json")
        .json(&req)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("failed to send assistant request to {}: {}", url, e))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(anyhow::anyhow!("assistant API returned {}: {}", status, body));
    }

    let parsed: Value = serde_json::from_str(&body).context("failed to parse assistant JSON")?;

    // Map expected fields into AssistResponse where possible
    let content = parsed.get("content").and_then(|v| v.as_str()).map(|s| s.to_string());
    let assistant_id = parsed.get("assistant_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let thread_id = parsed.get("thread_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let status = parsed.get("status").and_then(|v| v.as_str()).map(|s| s.to_string());

    let (command, explanation, error) = if let Some(content) = content.as_deref() {
        if let Ok(json) = serde_json::from_str::<Value>(content) {
            (
                json.get("command").and_then(|v| v.as_str()).map(|s| s.to_string()),
                json.get("explanation").and_then(|v| v.as_str()).map(|s| s.to_string()),
                json.get("error").and_then(|v| v.as_str()).map(|s| s.to_string()),
            )
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    Ok(AssistResponse { thread_id, assistant_id, content, status, command, explanation, error, raw: parsed })
}
