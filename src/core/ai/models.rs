use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub repetition_penalty: Option<f32>,
    pub reasoning_enabled: Option<bool>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AiResponse {
    pub answer: String,
    pub reasoning: Option<String>,
}
