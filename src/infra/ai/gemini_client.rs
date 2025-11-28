// =============================================================================
// GEMINI CLIENT - Google AI Studio API Integration
// =============================================================================
//
// This module provides an implementation of the `AiProvider` trait that
// communicates with Google's Gemini API (https://ai.google.dev/gemini-api/docs).
//
// **Key Differences from OpenRouter:**
// - Authentication: API key is passed as a query parameter (`?key=API_KEY`)
//   rather than a Bearer token in the Authorization header.
// - Request format: Uses `contents[]` with nested `parts`, and `systemInstruction`
//   is a separate top-level field (not a message with role "system").
// - Response format: Content is at `candidates[0].content.parts[0].text`.
//
// **Supported Models (as of late 2024):**
// - `gemini-2.5-flash` - Fast, balanced model (recommended for most use cases)
// - `gemini-2.5-flash-lite` - Fastest, most cost-efficient
// - `gemini-2.5-pro` - Most capable for complex reasoning
// - `gemini-3-pro` - Latest flagship model
//
// **Environment Variables:**
// - `GEMINI_API_KEY` - Your API key from https://aistudio.google.com/apikey

use crate::core::ai::{
    models::{AiConfig, AiMessage, AiProviderResponse},
    AiProvider,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

// =============================================================================
// GEMINI API DATA STRUCTURES
// =============================================================================
//
// These structs model the Gemini API request/response format.
// See: https://ai.google.dev/api/generate-content

/// A single part of content. Gemini uses a "parts" array to support
/// multimodal content (text, images, etc.). For text-only, we just use `text`.
#[derive(Debug, Serialize, Deserialize)]
struct Part {
    text: String,
}

/// Represents a message in the conversation. Maps to our `AiMessage` but
/// uses Gemini's expected format with `parts` array.
#[derive(Debug, Serialize, Deserialize)]
struct Content {
    /// Role: "user" or "model" (Gemini uses "model" instead of "assistant")
    role: String,
    /// Array of content parts (text, images, etc.)
    parts: Vec<Part>,
}

/// Generation configuration options that control the model's output.
/// See: https://ai.google.dev/api/generate-content#generationconfig
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    /// Controls randomness. Range: [0.0, 2.0]. Higher = more creative.
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,

    /// Maximum number of tokens to generate in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,

    /// Nucleus sampling: considers tokens with top_p cumulative probability.
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,

    /// Top-k sampling: considers only the top k most likely tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,

    /// Configuration for thinking/reasoning (Gemini 2.5+).
    /// This is nested inside generationConfig per the API spec.
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_config: Option<ThinkingConfig>,
}

/// Configuration for thinking/reasoning features (Gemini 2.5+).
/// When enabled, the model will show its reasoning process.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingConfig {
    /// Whether to include thought process in response.
    #[serde(skip_serializing_if = "Option::is_none")]
    include_thoughts: Option<bool>,

    /// Optional budget for thinking tokens.
    /// Set to 0 to disable thinking, -1 for dynamic thinking.
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_budget: Option<i32>,
}

/// The request body sent to the Gemini generateContent endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentRequest {
    /// The conversation history and current prompt.
    contents: Vec<Content>,

    /// System instruction (optional). This is separate from the conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,

    /// Configuration for generation parameters (includes thinkingConfig).
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

/// A candidate response from the model.
#[derive(Debug, Deserialize)]
struct Candidate {
    /// The generated content.
    content: Content,

    /// Why the model stopped generating (e.g., "STOP", "MAX_TOKENS").
    #[serde(rename = "finishReason")]
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

/// Token usage metadata for the request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageMetadata {
    /// Number of tokens in the prompt.
    #[allow(dead_code)]
    prompt_token_count: Option<u32>,

    /// Number of tokens in the response.
    #[allow(dead_code)]
    candidates_token_count: Option<u32>,

    /// Total tokens used.
    #[allow(dead_code)]
    total_token_count: Option<u32>,
}

/// The response from the Gemini generateContent endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentResponse {
    /// List of candidate responses. Usually just one.
    candidates: Option<Vec<Candidate>>,

    /// Token usage statistics.
    #[allow(dead_code)]
    usage_metadata: Option<UsageMetadata>,
}

/// Error response from the Gemini API.
#[derive(Debug, Deserialize)]
struct GeminiErrorDetail {
    message: String,
    #[allow(dead_code)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorResponse {
    error: GeminiErrorDetail,
}

// =============================================================================
// GEMINI CLIENT IMPLEMENTATION
// =============================================================================

/// Client for interacting with Google's Gemini API.
///
/// # Example
/// ```ignore
/// let client = GeminiClient::new("your-api-key".to_string());
/// let messages = vec![AiMessage {
///     role: "user".to_string(),
///     content: "Hello!".to_string(),
/// }];
/// let config = AiConfig {
///     model: "gemini-2.5-flash".to_string(),
///     temperature: 0.7,
///     ..Default::default()
/// };
/// let response = client.chat_complete(&messages, &config).await?;
/// ```
pub struct GeminiClient {
    /// HTTP client for making requests.
    client: Client,

    /// API key for authentication.
    api_key: String,
}

impl GeminiClient {
    /// Creates a new Gemini client with the given API key.
    ///
    /// # Arguments
    /// * `api_key` - Your Gemini API key from https://aistudio.google.com/apikey
    ///
    /// # Example
    /// ```ignore
    /// let client = GeminiClient::new(std::env::var("GEMINI_API_KEY")?);
    /// ```
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    /// Converts our generic `AiMessage` to Gemini's `Content` format.
    ///
    /// Key transformations:
    /// - "assistant" role → "model" (Gemini's terminology)
    /// - "system" messages are filtered out (handled separately)
    fn convert_message(msg: &AiMessage) -> Content {
        // Gemini uses "model" instead of "assistant"
        let role = match msg.role.as_str() {
            "assistant" => "model".to_string(),
            other => other.to_string(),
        };

        Content {
            role,
            parts: vec![Part {
                text: msg.content.clone(),
            }],
        }
    }
}

#[async_trait]
impl AiProvider for GeminiClient {
    /// Sends a chat completion request to the Gemini API.
    ///
    /// # Arguments
    /// * `messages` - The conversation history. System messages are extracted
    ///   and sent as `systemInstruction`.
    /// * `config` - Configuration including model name, temperature, etc.
    ///
    /// # Returns
    /// An `AiProviderResponse` containing the generated text and optional thinking.
    async fn chat_complete(
        &self,
        messages: &[AiMessage],
        config: &AiConfig,
    ) -> Result<AiProviderResponse, Box<dyn Error + Send + Sync>> {
        // Build the URL with API key as query parameter
        // Format: https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            config.model, self.api_key
        );

        // Separate system messages from conversation
        // Gemini handles system instructions differently - they're a separate field
        let system_instruction: Option<Content> =
            messages
                .iter()
                .find(|m| m.role == "system")
                .map(|m| Content {
                    role: "user".to_string(), // System instruction uses "user" role internally
                    parts: vec![Part {
                        text: m.content.clone(),
                    }],
                });

        // Convert non-system messages to Gemini format
        let contents: Vec<Content> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(Self::convert_message)
            .collect();

        // Build thinking config if reasoning is enabled
        // NOTE: thinkingConfig is only supported by Gemini 2.5+ models.
        // We check the model name to avoid sending it to unsupported models.
        let supports_thinking = config.model.contains("2.5") || config.model.contains("gemini-3");

        let thinking_config = if supports_thinking {
            config.reasoning_enabled.and_then(|enabled| {
                if enabled {
                    // Map reasoning effort to thinking budget
                    // "low" = smaller budget, "high" = larger budget
                    // Use -1 for dynamic thinking (model decides)
                    let thinking_budget = config.reasoning_effort.as_ref().map(|effort| {
                        match effort.to_lowercase().as_str() {
                            "low" => 1024,
                            "medium" => 4096,
                            "high" => 16384,
                            _ => -1, // Default to dynamic
                        }
                    });

                    Some(ThinkingConfig {
                        include_thoughts: Some(true),
                        thinking_budget,
                    })
                } else {
                    // Explicitly disable thinking by setting budget to 0
                    Some(ThinkingConfig {
                        include_thoughts: Some(false),
                        thinking_budget: Some(0),
                    })
                }
            })
        } else {
            // Model doesn't support thinking, don't send the config
            None
        };

        // Build generation config (includes thinking_config for 2.5+ models)
        let generation_config = GenerationConfig {
            temperature: Some(config.temperature),
            max_output_tokens: config.max_tokens,
            top_p: config.top_p,
            top_k: None,
            thinking_config,
        };

        // Build the request
        let request = GenerateContentRequest {
            contents,
            system_instruction,
            generation_config: Some(generation_config),
        };

        // Log request for debugging (be careful not to log the API key!)
        tracing::debug!(
            "Gemini request to model {}: {} messages",
            config.model,
            messages.len()
        );

        // Send the request
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        // Check for HTTP errors
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;

            // Try to parse as Gemini error response for better error messages
            if let Ok(error_response) = serde_json::from_str::<GeminiErrorResponse>(&error_text) {
                return Err(format!(
                    "Gemini API error ({}): {}",
                    status, error_response.error.message
                )
                .into());
            }

            return Err(format!("Gemini API error: {} - {}", status, error_text).into());
        }

        // Parse the response
        let response_json: GenerateContentResponse = response.json().await?;

        // Extract thinking and content from the response parts.
        //
        // When thinking/reasoning is enabled, Gemini returns multiple parts:
        // - If there are 2+ parts: parts[0..n-1] are thinking, parts[n-1] is the response
        // - If there's 1 part: it's just the response (no thinking)
        //
        // We extract thinking from all but the last part, and content from the last part.
        let parts = response_json
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .map(|c| &c.content.parts)
            .ok_or(
                "No content in Gemini response - the model may have been blocked by safety filters",
            )?;

        // Extract thinking: all parts except the last one, joined together
        let thinking = if parts.len() > 1 {
            let thinking_parts: Vec<&str> = parts[..parts.len() - 1]
                .iter()
                .map(|p| p.text.as_str())
                .collect();
            Some(thinking_parts.join("\n\n"))
        } else {
            None
        };

        // Extract content: the last part is always the actual response
        let content = parts
            .last()
            .map(|p| p.text.clone())
            .ok_or("No content parts in response")?;

        tracing::debug!(
            "Gemini response received: {} chars content, {} chars thinking",
            content.len(),
            thinking.as_ref().map(|t| t.len()).unwrap_or(0)
        );

        Ok(AiProviderResponse { content, thinking })
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_message_user() {
        let msg = AiMessage {
            role: "user".to_string(),
            content: "Hello!".to_string(),
        };

        let content = GeminiClient::convert_message(&msg);

        assert_eq!(content.role, "user");
        assert_eq!(content.parts.len(), 1);
        assert_eq!(content.parts[0].text, "Hello!");
    }

    #[test]
    fn test_convert_message_assistant_to_model() {
        let msg = AiMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
        };

        let content = GeminiClient::convert_message(&msg);

        // Gemini uses "model" instead of "assistant"
        assert_eq!(content.role, "model");
        assert_eq!(content.parts[0].text, "Hi there!");
    }

    #[test]
    fn test_generation_config_serialization() {
        let config = GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(1000),
            top_p: Some(0.9),
            top_k: None,
            thinking_config: None,
        };

        let json = serde_json::to_string(&config).unwrap();

        // Check camelCase serialization
        assert!(json.contains("\"temperature\""));
        assert!(json.contains("\"maxOutputTokens\""));
        assert!(json.contains("\"topP\""));
        // top_k should be skipped because it's None
        assert!(!json.contains("topK"));
        // thinking_config should be skipped because it's None
        assert!(!json.contains("thinkingConfig"));
    }
}
