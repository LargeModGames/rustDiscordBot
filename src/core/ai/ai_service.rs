use super::models::{
    AiConfig, AiMessage, AiProviderResponse, AiResponse, AiResponseWithMeta, AiTool, Citation,
    FunctionCall,
};
use async_trait::async_trait;
use std::error::Error;

// =============================================================================
// AI PROVIDER TRAIT
// =============================================================================

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

// =============================================================================
// FUNCTION CALL HANDLER TRAIT
// =============================================================================
//
// When the AI wants to call a function (like reading a Google Doc), we need
// to execute it and return the result. This trait defines the interface for
// handling those calls.

/// Handler for executing function calls from the AI model.
///
/// Implement this trait to provide custom function execution logic.
/// The handler receives the function name and arguments, and should return
/// a JSON result that will be sent back to the model.
#[async_trait]
pub trait FunctionCallHandler: Send + Sync {
    /// Executes a function call and returns the result as a JSON value.
    ///
    /// # Arguments
    /// * `name` - The name of the function to call
    /// * `args` - The arguments passed by the model (as JSON)
    ///
    /// # Returns
    /// The result of the function as a JSON value, or an error message.
    async fn handle_function_call(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String>;

    /// Returns the list of functions this handler can execute.
    fn supported_functions(&self) -> Vec<String>;
}

// =============================================================================
// AI SERVICE
// =============================================================================

pub struct AiService<P: AiProvider> {
    provider: P,
    system_prompt: String,
    config: AiConfig,
    /// Optional function call handler for executing tool calls
    function_handler: Option<Box<dyn FunctionCallHandler>>,
    /// Maximum number of tool call iterations to prevent infinite loops
    max_tool_iterations: usize,
}

impl<P: AiProvider> AiService<P> {
    pub fn new(provider: P, system_prompt: String, config: AiConfig) -> Self {
        Self {
            provider,
            system_prompt,
            config,
            function_handler: None,
            max_tool_iterations: 3,
        }
    }

    /// Creates a new AiService with a function call handler.
    ///
    /// Use this when you want the AI to be able to call functions like
    /// reading Google Docs, searching databases, etc.
    pub fn with_function_handler(
        provider: P,
        system_prompt: String,
        config: AiConfig,
        handler: Box<dyn FunctionCallHandler>,
    ) -> Self {
        Self {
            provider,
            system_prompt,
            config,
            function_handler: Some(handler),
            max_tool_iterations: 3,
        }
    }

    /// Sets the function call handler after construction.
    #[allow(dead_code)]
    pub fn set_function_handler(&mut self, handler: Box<dyn FunctionCallHandler>) {
        self.function_handler = Some(handler);
    }

    /// Returns the current tools configuration.
    #[allow(dead_code)]
    pub fn tools(&self) -> Option<&Vec<AiTool>> {
        self.config.tools.as_ref()
    }

    /// Updates the tools configuration.
    #[allow(dead_code)]
    pub fn set_tools(&mut self, tools: Option<Vec<AiTool>>) {
        self.config.tools = tools;
    }

    pub async fn chat(
        &self,
        context_messages: &[AiMessage],
    ) -> Result<AiResponse, Box<dyn Error + Send + Sync>> {
        let response_with_meta = self.chat_with_metadata(context_messages).await?;
        Ok(AiResponse {
            answer: response_with_meta.answer,
            reasoning: response_with_meta.reasoning,
        })
    }

    /// Sends a chat request and returns an extended response with metadata.
    ///
    /// This method extracts citations from grounding metadata when the model
    /// uses Google Search, making them available for display in Discord.
    ///
    /// # Arguments
    /// * `context_messages` - The conversation context (user messages, etc.)
    ///
    /// # Returns
    /// `AiResponseWithMeta` containing the answer, reasoning, citations, and confidence.
    pub async fn chat_with_metadata(
        &self,
        context_messages: &[AiMessage],
    ) -> Result<AiResponseWithMeta, Box<dyn Error + Send + Sync>> {
        // Build messages for API: System Prompt + Context
        let mut messages = Vec::new();
        messages.push(AiMessage {
            role: "system".to_string(),
            content: self.system_prompt.clone(),
        });
        messages.extend(context_messages.iter().cloned());

        // Call provider - now returns AiProviderResponse with thinking and content
        let mut provider_response = self.provider.chat_complete(&messages, &self.config).await?;

        // Handle function calls with multi-step support (up to max_tool_iterations rounds)
        let mut iteration = 0;
        loop {
            // Check if we have function calls to process
            let Some(ref function_calls) = provider_response.function_calls else {
                break;
            };
            let Some(ref handler) = self.function_handler else {
                break;
            };

            // Check if we've exceeded max iterations to prevent infinite loops
            if iteration >= self.max_tool_iterations {
                tracing::warn!(
                    "Reached max tool iterations ({}), stopping tool calls",
                    self.max_tool_iterations
                );
                break;
            }

            tracing::debug!(
                "Tool call iteration {} of max {}",
                iteration + 1,
                self.max_tool_iterations
            );

            // Clone function_calls to avoid borrow issues when reassigning provider_response
            let function_calls_owned = function_calls.clone();

            // Execute each function call and collect results
            let function_results =
                Self::execute_function_calls(&function_calls_owned, handler.as_ref()).await;

            // If we got function calls, we need to send results back and get next response
            if !function_results.is_empty() {
                // Update messages to include the function call exchange for next iteration
                let function_call_summary: Vec<String> = function_calls_owned
                    .iter()
                    .map(|fc| format!("Called {}({})", fc.name, fc.args))
                    .collect();
                messages.push(AiMessage {
                    role: "assistant".to_string(),
                    content: format!(
                        "I need to call some functions: {}",
                        function_call_summary.join(", ")
                    ),
                });
                messages.push(AiMessage {
                    role: "user".to_string(),
                    content: Self::format_function_results(&function_results),
                });

                provider_response = self
                    .continue_with_function_results(
                        &messages,
                        &function_calls_owned,
                        &function_results,
                        iteration < self.max_tool_iterations - 1, // allow more tools unless last iteration
                    )
                    .await?;
            } else {
                break;
            }

            iteration += 1;
        }

        // Parse response for XML tags (some models use <answer>/<rationale> tags)
        let (answer, xml_reasoning) = self.parse_response(&provider_response.content);

        // Extract citations from grounding metadata (before moving thinking)
        let citations = Self::extract_citations(&provider_response);

        // Prefer provider's built-in thinking (Gemini) over XML-parsed reasoning
        // This ensures we get the native thinking experience when available
        let reasoning = provider_response.thinking.or(xml_reasoning);

        // Note: confidence is not currently provided by Gemini grounding metadata
        // This field is reserved for future use or custom provider implementations
        let confidence = None;

        Ok(AiResponseWithMeta {
            answer,
            reasoning,
            citations,
            confidence,
        })
    }

    /// Extracts citations from the provider response's grounding metadata.
    fn extract_citations(provider_response: &AiProviderResponse) -> Vec<Citation> {
        let Some(ref grounding) = provider_response.grounding_metadata else {
            return Vec::new();
        };

        grounding
            .web_sources
            .iter()
            .map(|source| Citation {
                title: source.title.clone(),
                url: source.uri.clone(),
            })
            .collect()
    }

    /// Executes function calls and returns results.
    /// Each result is a tuple of (name, result, success).
    async fn execute_function_calls(
        function_calls: &[FunctionCall],
        handler: &dyn FunctionCallHandler,
    ) -> Vec<(String, serde_json::Value, bool)> {
        let mut results = Vec::new();

        for call in function_calls {
            tracing::info!(
                "Executing function call: {} with args: {}",
                call.name,
                call.args
            );

            match handler.handle_function_call(&call.name, &call.args).await {
                Ok(result) => {
                    tracing::debug!("Function {} returned: {:?}", call.name, result);
                    results.push((call.name.clone(), result, true));
                }
                Err(e) => {
                    // Use warn instead of error - failed function calls are not fatal
                    // and we continue processing other calls
                    tracing::warn!("Function {} failed: {}", call.name, e);
                    results.push((call.name.clone(), serde_json::json!({ "error": e }), false));
                }
            }
        }

        results
    }

    /// Formats function results using structured XML-like delimiters.
    fn format_function_results(results: &[(String, serde_json::Value, bool)]) -> String {
        let results_text: Vec<String> = results
            .iter()
            .map(|(name, result, success)| {
                let json_str =
                    serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string());
                format!(
                    "<function_result name=\"{}\" success=\"{}\">\n{}\n</function_result>",
                    name, success, json_str
                )
            })
            .collect();

        format!(
            "Here are the function results. Please use this information to answer my question:\n\n{}",
            results_text.join("\n\n")
        )
    }

    /// Continues the conversation with function call results.
    async fn continue_with_function_results(
        &self,
        original_messages: &[AiMessage],
        function_calls: &[FunctionCall],
        results: &[(String, serde_json::Value, bool)],
        allow_more_tools: bool,
    ) -> Result<AiProviderResponse, Box<dyn Error + Send + Sync>> {
        // Build messages including the function call and results
        // The model made function calls, so we add:
        // 1. The assistant's function call message (as model role)
        // 2. The function results (as user role with function response format)

        let mut messages = original_messages.to_vec();

        // Add assistant message indicating function calls were made
        let function_call_summary: Vec<String> = function_calls
            .iter()
            .map(|fc| format!("Called {}({})", fc.name, fc.args))
            .collect();

        messages.push(AiMessage {
            role: "assistant".to_string(),
            content: format!(
                "I need to call some functions: {}",
                function_call_summary.join(", ")
            ),
        });

        // Add function results as user message with structured format
        messages.push(AiMessage {
            role: "user".to_string(),
            content: Self::format_function_results(results),
        });

        // Make another API call with the function results
        // Only disable tools on the last iteration to allow multi-step tool use
        let config = if allow_more_tools {
            self.config.clone()
        } else {
            let mut config_without_tools = self.config.clone();
            config_without_tools.tools = None;
            config_without_tools
        };

        self.provider.chat_complete(&messages, &config).await
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
