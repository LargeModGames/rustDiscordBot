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
}

#[derive(Debug, Clone)]
pub struct AiResponse {
    pub answer: String,
    pub reasoning: Option<String>,
}
