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

/// Response from an AI provider, containing the main content and optional thinking.
///
/// This struct is returned by `AiProvider::chat_complete()` to allow providers
/// to return both the main response AND any thinking/reasoning process separately.
/// This is particularly useful for Gemini 2.5+ models which have built-in thinking.
#[derive(Debug, Clone, Default)]
pub struct AiProviderResponse {
    /// The main response content from the model.
    pub content: String,

    /// Optional thinking/reasoning process from the model.
    /// For Gemini 2.5+, this is the model's internal reasoning.
    /// For OpenRouter with reasoning enabled, this may also be populated.
    pub thinking: Option<String>,
}

/// Final response after processing by AiService.
/// Contains the parsed answer and any reasoning (from thinking or XML tags).
#[derive(Debug, Clone)]
pub struct AiResponse {
    pub answer: String,
    pub reasoning: Option<String>,
}
