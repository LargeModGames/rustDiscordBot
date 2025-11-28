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
// **Built-in Tools (Server-Side):**
// - `google_search` - Real-time web search with grounding metadata
// - `url_context` - Read content from URLs (web pages, PDFs, images)
//
// **Environment Variables:**
// - `GEMINI_API_KEY` - Your API key from https://aistudio.google.com/apikey

use crate::core::ai::{
    models::{
        AiConfig, AiMessage, AiProviderResponse, AiTool, FunctionCall, GroundingMetadata,
        ToolConfig, ToolMode, WebSource,
    },
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
/// multimodal content (text, images, function calls, etc.).
///
/// **Note:** We use `serde(default)` and `Option` for fields because different
/// response types include different combinations of fields.
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Part {
    /// Text content (for regular messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,

    /// Function call (when model wants to use a function).
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<GeminiFunctionCall>,

    /// Function response (when sending function results back).
    #[serde(skip_serializing_if = "Option::is_none")]
    function_response: Option<GeminiFunctionResponse>,
}

/// Function call requested by the model.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct GeminiFunctionCall {
    /// Name of the function to call.
    name: String,

    /// Arguments as JSON object.
    args: serde_json::Value,
}

/// Response from a function execution (sent back to the model).
#[derive(Debug, Serialize, Deserialize)]
struct GeminiFunctionResponse {
    /// Name of the function that was called.
    name: String,

    /// Result of the function execution as JSON.
    response: serde_json::Value,
}

/// Represents a message in the conversation. Maps to our `AiMessage` but
/// uses Gemini's expected format with `parts` array.
#[derive(Debug, Serialize, Deserialize)]
struct Content {
    /// Role: "user" or "model" (Gemini uses "model" instead of "assistant")
    role: String,
    /// Array of content parts (text, images, function calls, etc.)
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

// =============================================================================
// TOOL DEFINITIONS - REQUEST SIDE
// =============================================================================
//
// These structs define how to request tools from the Gemini API.
// See: https://ai.google.dev/gemini-api/docs/tools

/// A tool that can be used by the model.
///
/// Gemini supports several built-in tools (google_search, url_context)
/// as well as custom function declarations.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiTool {
    /// Google Search grounding tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    google_search: Option<GoogleSearchTool>,

    /// URL Context tool for reading web pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    url_context: Option<UrlContextTool>,

    /// Custom function declarations.
    #[serde(skip_serializing_if = "Option::is_none")]
    function_declarations: Option<Vec<GeminiFunctionDeclaration>>,
}

/// Google Search grounding tool configuration.
///
/// When enabled, the model can search Google for real-time information
/// and incorporate the results into its response. The API returns
/// `groundingMetadata` with citations and source links.
///
/// **Use cases:**
/// - Current events and news
/// - Fact-checking and verification
/// - Research on recent topics
/// - Any query requiring up-to-date information
///
/// **Example API request:**
/// ```json
/// {
///   "tools": [{
///     "google_search": {}
///   }]
/// }
/// ```
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct GoogleSearchTool {
    // Empty struct - just needs to be present to enable the tool
    // Future: May add configuration options like search restrictions
}

/// URL Context tool configuration.
///
/// Allows the model to read and analyze content from specified URLs.
/// The model can understand:
/// - HTML web pages
/// - PDF documents
/// - Plain text files
/// - Images (for visual analysis)
/// - JSON data
///
/// **⚠️ Limitations:**
/// - Google Docs/Sheets are NOT supported (use Files API or function calling)
/// - Private/authenticated pages won't work
/// - Very large pages may be truncated
/// - Some sites may block the crawler
///
/// **Example API request:**
/// ```json
/// {
///   "tools": [{
///     "url_context": {
///       "allowedUrls": ["https://example.com/page1", "https://example.com/page2"]
///     }
///   }]
/// }
/// ```
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct UrlContextTool {
    /// List of URLs the model is allowed to read.
    /// If empty, the model may read any URL it finds relevant.
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_urls: Option<Vec<String>>,
}

/// A custom function that the model can call.
///
/// Function calling lets you extend the model's capabilities with your own
/// logic. The model will return a structured request to call the function,
/// which you execute locally and send the results back.
///
/// **Workflow:**
/// 1. Define the function schema (name, description, parameters)
/// 2. Model analyzes when to call the function
/// 3. Model returns `functionCall` part with name and arguments
/// 4. You execute the function locally
/// 5. Send results back as `functionResponse`
/// 6. Model incorporates results into final response
///
/// **Example use cases:**
/// - Database queries
/// - API calls (weather, stocks, etc.)
/// - Reading Google Docs via Google Docs API
/// - Any custom business logic
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionDeclaration {
    /// Name of the function (used in function calls).
    name: String,

    /// Human-readable description of what the function does.
    /// The model uses this to decide when to call the function.
    description: String,

    /// JSON Schema describing the function's parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<GeminiFunctionParameters>,
}

/// JSON Schema for function parameters.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionParameters {
    /// Always "object" for function parameters.
    #[serde(rename = "type")]
    param_type: String,

    /// Map of parameter names to their schemas.
    properties: std::collections::HashMap<String, GeminiPropertySchema>,

    /// List of required parameter names.
    #[serde(skip_serializing_if = "Option::is_none")]
    required: Option<Vec<String>>,
}

/// Schema for a single property/parameter.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPropertySchema {
    /// JSON Schema type: "string", "number", "integer", "boolean", "array", "object"
    #[serde(rename = "type")]
    prop_type: String,

    /// Description of the parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    /// For enum types, the list of allowed values.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    enum_values: Option<Vec<String>>,
}

/// Configuration for how tools should be used.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiToolConfig {
    /// Function calling configuration.
    function_calling_config: FunctionCallingConfig,
}

/// Configuration for function calling behavior.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FunctionCallingConfig {
    /// Mode: "AUTO", "ANY", or "NONE"
    /// - AUTO: Model decides whether to use functions
    /// - ANY: Model must use one of the provided functions
    /// - NONE: Model should not use any functions
    mode: String,
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

    /// Tools available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,

    /// Configuration for tool usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<GeminiToolConfig>,
}

// =============================================================================
// RESPONSE STRUCTURES - INCLUDING GROUNDING METADATA
// =============================================================================

/// A candidate response from the model.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    /// The generated content.
    content: Content,

    /// Why the model stopped generating (e.g., "STOP", "MAX_TOKENS").
    #[allow(dead_code)]
    finish_reason: Option<String>,

    /// Grounding metadata when Google Search was used.
    grounding_metadata: Option<GeminiGroundingMetadata>,
}

/// Grounding metadata returned when Google Search tool is used.
///
/// This provides citations and source information for the model's response,
/// allowing you to show users where the information came from.
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GeminiGroundingMetadata {
    /// Search queries the model generated.
    #[serde(default)]
    search_entry_point: Option<SearchEntryPoint>,

    /// Web sources that contributed to the response.
    #[serde(default)]
    grounding_chunks: Vec<GeminiGroundingChunk>,

    /// Support for specific parts of the response.
    #[serde(default)]
    grounding_supports: Vec<GroundingSupport>,

    /// Retrieval metadata (for URL context).
    #[serde(default)]
    retrieval_metadata: Option<RetrievalMetadata>,
}

/// Search entry point - the query used for search.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SearchEntryPoint {
    /// The search query that was used.
    rendered_content: Option<String>,
}

/// A chunk of grounded content from a web source.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GeminiGroundingChunk {
    /// Web source information.
    web: Option<WebChunk>,
}

/// Web source information in a grounding chunk.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WebChunk {
    /// URI of the source.
    uri: Option<String>,

    /// Title of the source page.
    title: Option<String>,
}

/// Support information linking response parts to sources.
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GroundingSupport {
    /// Indices into grounding_chunks that support this text.
    #[serde(default)]
    grounding_chunk_indices: Vec<usize>,

    /// Confidence scores for the support.
    #[serde(default)]
    confidence_scores: Vec<f64>,
}

/// Metadata about URL retrieval.
#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RetrievalMetadata {
    /// URLs that were successfully retrieved.
    #[serde(default)]
    google_search_dynamic_retrieval_score: Option<f64>,
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

    /// Creates a Part with just text content.
    fn text_part(text: String) -> Part {
        Part {
            text: Some(text),
            function_call: None,
            function_response: None,
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
            parts: vec![Self::text_part(msg.content.clone())],
        }
    }

    /// Converts our core `AiTool` types to Gemini's tool format.
    ///
    /// This handles the translation between our platform-agnostic tool
    /// representation and Gemini's specific API format.
    fn convert_tools(tools: &[AiTool]) -> Vec<GeminiTool> {
        let mut gemini_tools = Vec::new();

        // Group tools by type - Gemini expects each tool type as a separate object
        let mut has_google_search = false;
        let mut url_context_urls: Vec<String> = Vec::new();
        let mut function_declarations: Vec<GeminiFunctionDeclaration> = Vec::new();

        for tool in tools {
            match tool {
                AiTool::GoogleSearch => {
                    has_google_search = true;
                }
                AiTool::UrlContext { urls } => {
                    url_context_urls.extend(urls.clone());
                }
                AiTool::FunctionDeclaration(func_def) => {
                    function_declarations.push(GeminiFunctionDeclaration {
                        name: func_def.name.clone(),
                        description: func_def.description.clone(),
                        parameters: Some(GeminiFunctionParameters {
                            param_type: func_def.parameters.param_type.clone(),
                            properties: func_def
                                .parameters
                                .properties
                                .iter()
                                .map(|(name, prop)| {
                                    (
                                        name.clone(),
                                        GeminiPropertySchema {
                                            prop_type: prop.prop_type.clone(),
                                            description: prop.description.clone(),
                                            enum_values: prop.enum_values.clone(),
                                        },
                                    )
                                })
                                .collect(),
                            required: if func_def.parameters.required.is_empty() {
                                None
                            } else {
                                Some(func_def.parameters.required.clone())
                            },
                        }),
                    });
                }
            }
        }

        // NOTE: Gemini currently rejects requests that mix built-in tools
        // (google_search/url_context) with function calling (function_declarations).
        // To avoid the 400 Bad Request error "Tool use with function calling is
        // unsupported", prefer custom function_declarations over built-in tools.
        // If function_declarations are present, we omit google_search/url_context.
        // The default behavior is to avoid mixing built-in tools and custom
        // function declarations, because Gemini may return 400 when mixing
        // them. You can override this behavior by setting
        // GEMINI_ALLOW_MIXED_TOOLS_AND_FUNCTIONS=true in the environment.
        let allow_mixed_tools = std::env::var("GEMINI_ALLOW_MIXED_TOOLS_AND_FUNCTIONS")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        if !function_declarations.is_empty() && !allow_mixed_tools {
            tracing::warn!("Gemini request: function_declarations present - omitting built-in tools (google_search/url_context) to avoid API error");
            gemini_tools.push(GeminiTool {
                google_search: None,
                url_context: None,
                function_declarations: Some(function_declarations),
            });
        } else {
            // Add Google Search tool if requested
            if has_google_search {
                gemini_tools.push(GeminiTool {
                    google_search: Some(GoogleSearchTool {}),
                    url_context: None,
                    function_declarations: None,
                });
            }

            // Add URL Context tool if any URLs were specified
            if !url_context_urls.is_empty() {
                gemini_tools.push(GeminiTool {
                    google_search: None,
                    url_context: Some(UrlContextTool {
                        allowed_urls: Some(url_context_urls),
                    }),
                    function_declarations: None,
                });
            }

            // If function declarations are present and mixing is allowed, add them too
            if !function_declarations.is_empty() {
                gemini_tools.push(GeminiTool {
                    google_search: None,
                    url_context: None,
                    function_declarations: Some(function_declarations),
                });
            }
        }

        gemini_tools
    }

    /// Converts our core `ToolConfig` to Gemini's tool config format.
    fn convert_tool_config(tool_config: &ToolConfig) -> GeminiToolConfig {
        let mode = match tool_config.mode {
            ToolMode::Auto => "AUTO".to_string(),
            ToolMode::Required => "ANY".to_string(),
            ToolMode::None => "NONE".to_string(),
        };

        GeminiToolConfig {
            function_calling_config: FunctionCallingConfig { mode },
        }
    }

    /// Converts Gemini's grounding metadata to our core format.
    fn convert_grounding_metadata(metadata: &GeminiGroundingMetadata) -> GroundingMetadata {
        let search_queries = metadata
            .search_entry_point
            .as_ref()
            .and_then(|sep| sep.rendered_content.clone())
            .map(|q| vec![q])
            .unwrap_or_default();

        let web_sources = metadata
            .grounding_chunks
            .iter()
            .filter_map(|chunk| {
                chunk.web.as_ref().and_then(|web| {
                    web.uri.as_ref().map(|uri| WebSource {
                        uri: uri.clone(),
                        title: web.title.clone(),
                    })
                })
            })
            .collect();

        GroundingMetadata {
            search_queries,
            web_sources,
            grounding_chunks: Vec::new(), // TODO: Parse grounding chunks with content
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
                    parts: vec![Self::text_part(m.content.clone())],
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

        // Convert tools to Gemini format if provided
        let tools = config
            .tools
            .as_ref()
            .map(|t| Self::convert_tools(t))
            .filter(|t| !t.is_empty());

        // Convert tool config if provided
        let tool_config = config.tool_config.as_ref().map(Self::convert_tool_config);

        // Build the request
        let request = GenerateContentRequest {
            contents,
            system_instruction,
            generation_config: Some(generation_config),
            tools,
            tool_config,
        };

        // Log request for debugging (be careful not to log the API key!)
        tracing::debug!(
            "Gemini request to model {}: {} messages, tools: {:?}",
            config.model,
            messages.len(),
            config.tools.as_ref().map(|t| t.len()).unwrap_or(0)
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

        // Get the first candidate (usually the only one)
        let candidate = response_json
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .ok_or(
                "No content in Gemini response - the model may have been blocked by safety filters",
            )?;

        let parts = &candidate.content.parts;

        // Extract function calls if the model wants to call any functions
        //
        // When function calling is used, the model may return parts with
        // `function_call` instead of text. We extract these separately.
        let function_calls: Vec<FunctionCall> = parts
            .iter()
            .filter_map(|p| p.function_call.as_ref())
            .map(|fc| FunctionCall {
                name: fc.name.clone(),
                args: fc.args.clone(),
            })
            .collect();

        let function_calls = if function_calls.is_empty() {
            None
        } else {
            Some(function_calls)
        };

        // Extract grounding metadata if Google Search was used
        let grounding_metadata = candidate
            .grounding_metadata
            .as_ref()
            .map(Self::convert_grounding_metadata);

        // Extract text parts only (filter out function calls)
        let text_parts: Vec<&Part> = parts.iter().filter(|p| p.text.is_some()).collect();

        // Extract thinking and content from the response parts.
        //
        // When thinking/reasoning is enabled, Gemini returns multiple parts:
        // - If there are 2+ parts: parts[0..n-1] are thinking, parts[n-1] is the response
        // - If there's 1 part: it's just the response (no thinking)
        //
        // We extract thinking from all but the last part, and content from the last part.
        let thinking = if text_parts.len() > 1 {
            let thinking_parts: Vec<&str> = text_parts[..text_parts.len() - 1]
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect();
            if thinking_parts.is_empty() {
                None
            } else {
                Some(thinking_parts.join("\n\n"))
            }
        } else {
            None
        };

        // Extract content: the last text part is always the actual response
        let content = text_parts
            .last()
            .and_then(|p| p.text.clone())
            .unwrap_or_default();

        tracing::debug!(
            "Gemini response received: {} chars content, {} chars thinking, {} function calls",
            content.len(),
            thinking.as_ref().map(|t| t.len()).unwrap_or(0),
            function_calls.as_ref().map(|f| f.len()).unwrap_or(0)
        );

        Ok(AiProviderResponse {
            content,
            thinking,
            grounding_metadata,
            url_context_metadata: None, // TODO: Parse URL context metadata when available
            function_calls,
        })
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
        assert_eq!(content.parts[0].text, Some("Hello!".to_string()));
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
        assert_eq!(content.parts[0].text, Some("Hi there!".to_string()));
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

    #[test]
    fn test_google_search_tool_conversion() {
        let tools = vec![AiTool::GoogleSearch];
        let gemini_tools = GeminiClient::convert_tools(&tools);

        assert_eq!(gemini_tools.len(), 1);
        assert!(gemini_tools[0].google_search.is_some());
        assert!(gemini_tools[0].url_context.is_none());
        assert!(gemini_tools[0].function_declarations.is_none());
    }

    #[test]
    fn test_url_context_tool_conversion() {
        let tools = vec![AiTool::UrlContext {
            urls: vec!["https://example.com".to_string()],
        }];
        let gemini_tools = GeminiClient::convert_tools(&tools);

        assert_eq!(gemini_tools.len(), 1);
        assert!(gemini_tools[0].google_search.is_none());
        assert!(gemini_tools[0].url_context.is_some());

        let url_context = gemini_tools[0].url_context.as_ref().unwrap();
        assert_eq!(
            url_context.allowed_urls,
            Some(vec!["https://example.com".to_string()])
        );
    }

    #[test]
    fn test_multiple_tools_conversion() {
        let tools = vec![
            AiTool::GoogleSearch,
            AiTool::UrlContext {
                urls: vec!["https://example.com".to_string()],
            },
        ];
        let gemini_tools = GeminiClient::convert_tools(&tools);

        // Each tool type gets its own entry
        assert_eq!(gemini_tools.len(), 2);
        assert!(gemini_tools[0].google_search.is_some());
        assert!(gemini_tools[1].url_context.is_some());
    }

    #[test]
    fn test_function_declarations_prefered_over_builtin_tools() {
        // If both GoogleSearch and FunctionDeclaration are present, the client will
        // prefer function_declarations and omit google_search/url_context to avoid
        // API errors from Gemini that reject mixing built-ins with functions.
        use crate::core::ai::models::{FunctionDef, FunctionParameters};
        let parameters = FunctionParameters {
            param_type: "object".to_string(),
            properties: std::collections::HashMap::new(),
            required: vec![],
        };
        let func_def = FunctionDef {
            name: "read_google_doc".to_string(),
            description: "Read a Google Doc".to_string(),
            parameters,
        };
        let tools = vec![AiTool::GoogleSearch, AiTool::FunctionDeclaration(func_def)];
        let gemini_tools = GeminiClient::convert_tools(&tools);

        assert_eq!(gemini_tools.len(), 1);
        assert!(gemini_tools[0].function_declarations.is_some());
        assert!(gemini_tools[0].google_search.is_none());
        assert!(gemini_tools[0].url_context.is_none());
    }

    #[test]
    fn test_allow_mixing_tools_and_functions_via_env() {
        // If env var GEMINI_ALLOW_MIXED_TOOLS_AND_FUNCTIONS=true, allow mixing
        // built-in tools and function_declarations. This is opt-in and not done
        // by default because Gemini may reject mixed requests.
        use std::env;
        env::set_var("GEMINI_ALLOW_MIXED_TOOLS_AND_FUNCTIONS", "true");
        use crate::core::ai::models::{FunctionDef, FunctionParameters};
        let parameters = FunctionParameters {
            param_type: "object".to_string(),
            properties: std::collections::HashMap::new(),
            required: vec![],
        };
        let func_def = FunctionDef {
            name: "read_google_doc".to_string(),
            description: "Read a Google Doc".to_string(),
            parameters,
        };
        let tools = vec![AiTool::GoogleSearch, AiTool::FunctionDeclaration(func_def)];
        let gemini_tools = GeminiClient::convert_tools(&tools);

        // Both tools should be present when mixing is allowed
        assert_eq!(gemini_tools.len(), 2);
        assert!(gemini_tools.iter().any(|t| t.google_search.is_some()));
        assert!(gemini_tools
            .iter()
            .any(|t| t.function_declarations.is_some()));

        // Clean up env var
        env::remove_var("GEMINI_ALLOW_MIXED_TOOLS_AND_FUNCTIONS");
    }
}
