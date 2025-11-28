use super::models::{AiConfig, AiMessage, AiProviderResponse, AiResponse};
use async_trait::async_trait;
use std::error::Error;

#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Sends a chat completion request to the AI provider.
    ///
    /// Returns an `AiProviderResponse` containing both the main content
    /// and optional thinking/reasoning from the model.
    async fn chat_complete(
        &self,
        messages: &[AiMessage],
        config: &AiConfig,
    ) -> Result<AiProviderResponse, Box<dyn Error + Send + Sync>>;
}

// Blanket implementation for Box<dyn AiProvider>
// This allows us to use trait objects in the AiService, enabling
// runtime switching between different AI providers (OpenRouter, Gemini, etc.)
#[async_trait]
impl AiProvider for Box<dyn AiProvider> {
    async fn chat_complete(
        &self,
        messages: &[AiMessage],
        config: &AiConfig,
    ) -> Result<AiProviderResponse, Box<dyn Error + Send + Sync>> {
        // Delegate to the inner provider
        (**self).chat_complete(messages, config).await
    }
}

pub struct AiService<P: AiProvider> {
    provider: P,
    system_prompt: String,
    config: AiConfig,
}

impl<P: AiProvider> AiService<P> {
    pub fn new(provider: P, system_prompt: String, config: AiConfig) -> Self {
        Self {
            provider,
            system_prompt,
            config,
        }
    }

    pub async fn chat(
        &self,
        context_messages: &[AiMessage],
    ) -> Result<AiResponse, Box<dyn Error + Send + Sync>> {
        // Build messages for API: System Prompt + Context
        let mut messages = Vec::new();
        messages.push(AiMessage {
            role: "system".to_string(),
            content: self.system_prompt.clone(),
        });
        messages.extend(context_messages.iter().cloned());

        // Call provider - now returns AiProviderResponse with thinking and content
        let provider_response = self.provider.chat_complete(&messages, &self.config).await?;

        // Parse response for XML tags (some models use <answer>/<rationale> tags)
        let (answer, xml_reasoning) = self.parse_response(&provider_response.content);

        // Prefer provider's built-in thinking (Gemini) over XML-parsed reasoning
        // This ensures we get the native thinking experience when available
        let reasoning = provider_response.thinking.or(xml_reasoning);

        Ok(AiResponse { answer, reasoning })
    }

    fn parse_response(&self, content: &str) -> (String, Option<String>) {
        let mut answer = content.to_string();
        let mut reasoning = None;

        if let Some(start_ans) = content.find("<answer>") {
            if let Some(end_ans) = content.find("</answer>") {
                if end_ans > start_ans {
                    answer = content[start_ans + 8..end_ans].trim().to_string();
                }
            }
        }

        if let Some(start_rat) = content.find("<rationale>") {
            if let Some(end_rat) = content.find("</rationale>") {
                if end_rat > start_rat {
                    reasoning = Some(content[start_rat + 11..end_rat].trim().to_string());
                }
            }
        }

        (answer, reasoning)
    }
}
