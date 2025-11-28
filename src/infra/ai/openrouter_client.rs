use crate::core::ai::{
    models::{AiConfig, AiMessage, AiProviderResponse},
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
    ) -> Result<AiProviderResponse, Box<dyn Error + Send + Sync>> {
        let url = "https://openrouter.ai/api/v1/chat/completions";

        let mut payload = json!({
            "model": config.model,
            "messages": messages,
            "temperature": config.temperature,
        });

        if let Some(max_tokens) = config.max_tokens {
            payload
                .as_object_mut()
                .unwrap()
                .insert("max_tokens".to_string(), json!(max_tokens));
        }

        if let Some(top_p) = config.top_p {
            payload
                .as_object_mut()
                .unwrap()
                .insert("top_p".to_string(), json!(top_p));
        }

        if let Some(repetition_penalty) = config.repetition_penalty {
            payload
                .as_object_mut()
                .unwrap()
                .insert("repetition_penalty".to_string(), json!(repetition_penalty));
        }

        if let Some(enabled) = config.reasoning_enabled {
            if enabled {
                let mut reasoning = serde_json::Map::new();
                reasoning.insert("enabled".to_string(), json!(true));

                if let Some(effort) = &config.reasoning_effort {
                    reasoning.insert("effort".to_string(), json!(effort));
                }

                payload.as_object_mut().unwrap().insert(
                    "reasoning".to_string(),
                    serde_json::Value::Object(reasoning),
                );
            }
        }

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header(
                "HTTP-Referer",
                "https://github.com/LargeModGames/rustDiscordBot",
            )
            .header("X-Title", "Rust Discord Bot")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("OpenRouter API error: {} - {}", status, text).into());
        }

        let response_json: serde_json::Value = response.json().await?;

        // Extract content from the response
        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("Failed to parse response content")?
            .to_string();

        // OpenRouter doesn't have separate thinking field in the same way,
        // so we return None for thinking (the XML parsing in AiService handles it)
        Ok(AiProviderResponse {
            content,
            thinking: None,
            grounding_metadata: None,
            url_context_metadata: None,
            function_calls: None,
        })
    }
}
