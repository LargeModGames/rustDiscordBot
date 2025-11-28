use serde::{Deserialize, Serialize};

// =============================================================================
// AI MESSAGE TYPES
// =============================================================================

/// Represents a single message in an AI conversation.
///
/// Messages have a role (user, assistant, system) and content.
/// This is a platform-agnostic representation that gets converted to
/// provider-specific formats (e.g., Gemini uses "model" instead of "assistant").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

// =============================================================================
// AI TOOLS - CORE ABSTRACTIONS
// =============================================================================
//
// Tools allow AI models to access external capabilities beyond text generation.
// This includes web search, URL reading, and custom function calling.
//
// **Why tools in the core layer?**
// Tools are part of the AI domain model - they define WHAT capabilities
// are available, not HOW they're implemented. The infra layer translates
// these to provider-specific formats (e.g., Gemini's `google_search` tool).

/// Represents a tool that can be used by an AI model.
///
/// Different AI providers support different tools. This enum provides a
/// unified interface that gets translated to provider-specific formats.
///
/// # Supported Tools by Provider
///
/// | Tool               | Gemini | OpenRouter | Notes                           |
/// |--------------------|--------|------------|---------------------------------|
/// | GoogleSearch       | ✅     | ❌         | Built-in, server-side           |
/// | UrlContext         | ✅     | ❌         | Reads web pages, PDFs, images   |
/// | FunctionDeclaration| ✅     | ✅         | Custom tools you define         |
///
/// # Example
/// ```ignore
/// let tools = vec![
///     AiTool::GoogleSearch,
///     AiTool::UrlContext { urls: vec!["https://example.com".into()] },
/// ];
/// ```
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AiTool {
    /// Google Search grounding - lets the model search the web for real-time info.
    ///
    /// When enabled, the model can autonomously search Google and incorporate
    /// results into its response. The response includes `grounding_metadata`
    /// with citations and source links.
    ///
    /// **Use cases:**
    /// - Current events and news
    /// - Fact-checking
    /// - Research on recent topics
    ///
    /// **Note:** This is a Gemini-specific tool executed server-side by Google.
    GoogleSearch,

    /// URL Context - lets the model read and analyze content from URLs.
    ///
    /// The model can fetch and understand content from web pages, including:
    /// - HTML pages
    /// - PDF documents
    /// - Plain text files
    /// - Images (for visual analysis)
    /// - JSON data
    ///
    /// **⚠️ Limitations:**
    /// - Google Docs/Sheets are NOT supported (use Files API instead)
    /// - Private/authenticated pages won't work
    /// - Very large pages may be truncated
    ///
    /// **Note:** This is a Gemini-specific tool executed server-side by Google.
    UrlContext {
        /// List of URLs the model should be able to read.
        /// These are passed to Gemini's URL Context tool.
        urls: Vec<String>,
    },

    /// Custom function declaration - define your own tools.
    ///
    /// Function calling lets you define custom capabilities that the model
    /// can use. The model returns structured JSON indicating which function
    /// to call with what arguments, and you execute it locally.
    ///
    /// **Flow:**
    /// 1. You define the function schema (name, description, parameters)
    /// 2. Model decides when to call the function and with what args
    /// 3. Model returns a "function call" response
    /// 4. You execute the function and send results back
    /// 5. Model incorporates results into final response
    ///
    /// **Use cases:**
    /// - Database queries
    /// - API calls (weather, stocks, etc.)
    /// - Reading from private sources (Google Docs via API)
    /// - Any custom logic you want the AI to trigger
    FunctionDeclaration(FunctionDef),
}

/// Definition of a custom function that can be called by the AI model.
///
/// This follows the JSON Schema specification for describing function
/// parameters. The model uses this information to understand when and
/// how to call your function.
///
/// # Example
/// ```ignore
/// FunctionDef {
///     name: "get_weather".to_string(),
///     description: "Get current weather for a location".to_string(),
///     parameters: FunctionParameters {
///         param_type: "object".to_string(),
///         properties: {
///             let mut props = HashMap::new();
///             props.insert("location".to_string(), PropertyDef {
///                 prop_type: "string".to_string(),
///                 description: Some("City name".to_string()),
///                 enum_values: None,
///             });
///             props
///         },
///         required: vec!["location".to_string()],
///     },
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    /// Name of the function (used in function calls).
    pub name: String,

    /// Human-readable description of what the function does.
    /// The model uses this to decide when to call the function.
    pub description: String,

    /// JSON Schema describing the function's parameters.
    pub parameters: FunctionParameters,
}

/// JSON Schema describing the parameters a function accepts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionParameters {
    /// Always "object" for function parameters.
    #[serde(rename = "type")]
    pub param_type: String,

    /// Map of parameter names to their definitions.
    pub properties: std::collections::HashMap<String, PropertyDef>,

    /// List of required parameter names.
    #[serde(default)]
    pub required: Vec<String>,
}

/// Definition of a single property/parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDef {
    /// JSON Schema type: "string", "number", "integer", "boolean", "array", "object"
    #[serde(rename = "type")]
    pub prop_type: String,

    /// Description of the parameter (helps the model understand how to use it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// For enum types, the list of allowed values.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
}

// =============================================================================
// AI CONFIGURATION
// =============================================================================

/// Configuration for an AI request.
///
/// This contains all the parameters that control how the AI model generates
/// its response, including model selection, temperature, and tool usage.
#[derive(Debug, Clone)]
pub struct AiConfig {
    /// The model identifier (e.g., "gemini-2.5-flash", "gpt-4", etc.)
    pub model: String,

    /// Controls randomness. Higher = more creative, lower = more focused.
    /// Range: 0.0 - 2.0 (varies by provider)
    pub temperature: f32,

    /// Maximum number of tokens to generate.
    pub max_tokens: Option<u32>,

    /// Nucleus sampling threshold.
    pub top_p: Option<f32>,

    /// Penalty for repeating tokens.
    pub repetition_penalty: Option<f32>,

    /// Whether to enable reasoning/thinking mode.
    pub reasoning_enabled: Option<bool>,

    /// Effort level for reasoning ("low", "medium", "high").
    pub reasoning_effort: Option<String>,

    /// Tools available to the model.
    ///
    /// **Important:** Not all tools are supported by all providers.
    /// - Gemini: GoogleSearch, UrlContext, FunctionDeclaration
    /// - OpenRouter: FunctionDeclaration only (depends on underlying model)
    pub tools: Option<Vec<AiTool>>,

    /// Tool configuration options.
    pub tool_config: Option<ToolConfig>,
}

/// Configuration for how tools should be used.
#[derive(Debug, Clone)]
pub struct ToolConfig {
    /// How the model should handle tool usage.
    pub mode: ToolMode,
}

/// Controls when/how the model uses tools.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ToolMode {
    /// Model decides whether to use tools (default).
    Auto,

    /// Model must use one of the provided tools.
    Required,

    /// Model should not use any tools (even if provided).
    None,
}

/// Response from an AI provider, containing the main content and optional thinking.
///
/// This struct is returned by `AiProvider::chat_complete()` to allow providers
/// to return both the main response AND any thinking/reasoning process separately.
/// This is particularly useful for Gemini 2.5+ models which have built-in thinking.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct AiProviderResponse {
    /// The main response content from the model.
    pub content: String,

    /// Optional thinking/reasoning process from the model.
    /// For Gemini 2.5+, this is the model's internal reasoning.
    /// For OpenRouter with reasoning enabled, this may also be populated.
    pub thinking: Option<String>,

    /// Grounding metadata from Google Search tool.
    /// Contains citations and source links when the model used web search.
    pub grounding_metadata: Option<GroundingMetadata>,

    /// URL context metadata.
    /// Contains information about URLs that were read by the model.
    pub url_context_metadata: Option<UrlContextMetadata>,

    /// Function calls requested by the model.
    /// When the model wants to use a custom function, it returns these.
    /// You should execute them and send results back.
    pub function_calls: Option<Vec<FunctionCall>>,
}

/// Metadata from Google Search grounding.
///
/// When the model uses the Google Search tool, it includes this metadata
/// to provide citations and sources for the information it found.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct GroundingMetadata {
    /// Search queries the model generated.
    pub search_queries: Vec<String>,

    /// Web sources that were used.
    pub web_sources: Vec<WebSource>,

    /// Grounding chunks (snippets of content from sources).
    pub grounding_chunks: Vec<GroundingChunk>,
}

/// A web source used in grounding.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct WebSource {
    /// URI of the source.
    pub uri: String,

    /// Title of the source page.
    pub title: Option<String>,
}

/// A chunk of grounded content from a source.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GroundingChunk {
    /// The content snippet.
    pub content: String,

    /// Source information for this chunk.
    pub source: Option<WebSource>,
}

/// Metadata from URL Context tool.
///
/// When the model reads URLs, this contains information about what was read.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct UrlContextMetadata {
    /// URLs that were successfully read.
    pub urls_read: Vec<String>,

    /// URLs that failed to be read (with error messages).
    pub urls_failed: Vec<(String, String)>,
}

/// A function call requested by the model.
///
/// When using function calling, the model may decide to call one of your
/// defined functions. This struct contains the function name and arguments.
///
/// **Workflow:**
/// 1. Model returns `FunctionCall` with name and args
/// 2. You execute the function locally
/// 3. Send the result back to the model as a function response message
/// 4. Model incorporates the result into its final answer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Name of the function to call.
    pub name: String,

    /// Arguments as a JSON object.
    /// You'll need to deserialize this based on your function's expected params.
    pub args: serde_json::Value,
}

/// Final response after processing by AiService.
/// Contains the parsed answer and any reasoning (from thinking or XML tags).
#[derive(Debug, Clone)]
pub struct AiResponse {
    pub answer: String,
    pub reasoning: Option<String>,
}
