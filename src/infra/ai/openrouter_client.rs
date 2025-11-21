use crate::core::ai::{
    models::{AiConfig, AiMessage},
    AiProvider,
};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::error::Error;

pub struct OpenRouterClient {
    client: Client,
    api_key: String,
}

impl OpenRouterClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }
}

#[async_trait]
impl AiProvider for OpenRouterClient {
    async fn chat_complete(
        &self,
        messages: &[AiMessage],
        config: &AiConfig,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let url = "https://openrouter.ai/api/v1/chat/completions";

        let payload = json!({
            "model": config.model,
            "messages": messages,
            "temperature": config.temperature,
            "max_tokens": config.max_tokens,
        });

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("OpenRouter API error: {} - {}", status, text).into());
        }

        let response_json: serde_json::Value = response.json().await?;

        // Extract content
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("Failed to parse response content")?
            .to_string();

        Ok(content)
    }
}
