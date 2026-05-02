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
        let system_prompt = std::env::var("BACKBOARD_PROMPT").ok().or_else(|| Some(default_system_prompt(&assistant_name)));

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

fn default_system_prompt(assistant_name: &str) -> String {
    format!(
        "You are {assistant_name}, a caesim command assistant. Convert the user's request into exactly one safe caesim CLI command. Follow these rules:\n\n- Output JSON only with keys: command, explanation.\n- If the request is ambiguous, choose the safest dry-run variant and explain the assumption.\n- Use caesim cut syntax only.\n- Prefer --dry-run unless the user explicitly asks to move files.\n- Use --rule for local rules: screenshots, duplicates, explicit, landscape, portrait.\n- Use --contains for image-label queries like food, cars, receipts.\n- Use --destination when the user names a target folder.\n- Never invent flags.\n- Never delete files.\n- If the request needs a path and none is given, default to the current directory or ask for clarification.\n- When a command is possible, return something like: {{\"command\":\"caesim cut ./photos --rule landscape --dry-run\",\"explanation\":\"Landscape filter in preview mode.\"}}\n- If no safe command is possible, return {{\"command\":null,\"explanation\":\"...\"}}.\n"
    )
}
