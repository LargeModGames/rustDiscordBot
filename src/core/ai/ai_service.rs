use super::models::{AiConfig, AiMessage, AiResponse};
use async_trait::async_trait;
use std::error::Error;

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn chat_complete(
        &self,
        messages: &[AiMessage],
        config: &AiConfig,
    ) -> Result<String, Box<dyn Error + Send + Sync>>;
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

        // Call provider
        let response_content = self.provider.chat_complete(&messages, &self.config).await?;

        // Parse response
        let (answer, reasoning) = self.parse_response(&response_content);

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
