use super::models::{AiConfig, AiMessage, AiResponse};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use tokio::sync::RwLock;

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
    // Map<ChannelId, History>
    history: RwLock<HashMap<u64, VecDeque<AiMessage>>>,
    system_prompt: String,
    config: AiConfig,
}

impl<P: AiProvider> AiService<P> {
    pub fn new(provider: P, system_prompt: String, config: AiConfig) -> Self {
        Self {
            provider,
            history: RwLock::new(HashMap::new()),
            system_prompt,
            config,
        }
    }

    pub async fn chat(
        &self,
        channel_id: u64,
        user_message: String,
    ) -> Result<AiResponse, Box<dyn Error + Send + Sync>> {
        let mut history_lock = self.history.write().await;
        let channel_history = history_lock
            .entry(channel_id)
            .or_insert_with(|| VecDeque::with_capacity(20));

        // Add user message
        channel_history.push_back(AiMessage {
            role: "user".to_string(),
            content: user_message.clone(),
        });

        // Trim history if needed (keep last 20)
        while channel_history.len() > 20 {
            channel_history.pop_front();
        }

        // Build messages for API: System Prompt + History
        let mut messages = Vec::new();
        messages.push(AiMessage {
            role: "system".to_string(),
            content: self.system_prompt.clone(),
        });
        messages.extend(channel_history.iter().cloned());

        // Call provider
        let response_content = self.provider.chat_complete(&messages, &self.config).await?;

        // Parse response
        let (answer, reasoning) = self.parse_response(&response_content);

        // Add assistant response to history
        channel_history.push_back(AiMessage {
            role: "assistant".to_string(),
            content: response_content.clone(),
        });

        // Trim again just in case
        while channel_history.len() > 20 {
            channel_history.pop_front();
        }

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
