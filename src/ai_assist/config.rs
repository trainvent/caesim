use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantConfig {
    pub assistant_name: String,
    pub api_base: String,
    pub assistant_id: Option<String>,
    pub thread_id: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub system_prompt: Option<String>,
}

impl AssistantConfig {
    pub fn load_from_env() -> Result<Self> {
        let assistant_name = std::env::var("BACKBOARD_API_NAME").unwrap_or_else(|_| "caesim".to_string());
        let api_base = std::env::var("BACKBOARD_API_BASE").unwrap_or_else(|_| "https://app.backboard.io/api".to_string());
        let assistant_id = std::env::var("BACKBOARD_ASSISTANT_ID").ok();
        let thread_id = std::env::var("BACKBOARD_THREAD_ID").ok();
        let model = std::env::var("BACKBOARD_MODEL").ok();
        let temperature = std::env::var("BACKBOARD_TEMPERATURE").ok().and_then(|s| s.parse().ok());
        let system_prompt = std::env::var("BACKBOARD_PROMPT").ok();

        Ok(AssistantConfig {
            assistant_name,
            api_base,
            assistant_id,
            thread_id,
            model,
            temperature,
            system_prompt,
        })
    }
}
