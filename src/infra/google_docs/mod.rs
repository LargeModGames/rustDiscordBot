// =============================================================================
// GOOGLE DOCS MODULE
// =============================================================================
//
// This module provides integration with Google Docs for the Discord bot.
// Since Gemini's URL Context tool does NOT support Google Docs directly,
// we need to fetch document content ourselves using the Google Docs API.
//
// **Architecture:**
// This module lives in the infra layer because it handles external I/O
// (HTTP requests to Google APIs). The core layer only knows about
// "document content" - it doesn't care where it comes from.
//
// **Authentication Options:**
// 1. **API Key** (simplest): Only works for publicly shared documents
// 2. **Service Account** (recommended for bots): Can access private docs
//    if the docs are shared with the service account email
// 3. **OAuth 2.0**: For user-delegated access (not typical for bots)
//
// **Usage Patterns:**
//
// Pattern 1: Pre-fetch and inject into system prompt
// ```ignore
// let content = google_docs_client.get_document_text(doc_id).await?;
// let enhanced_prompt = format!("{}\n\nProject Info:\n{}", base_prompt, content);
// ```
//
// Pattern 2: As a function tool (Gemini calls it when needed)
// ```ignore
// let handler = GoogleDocsFunctionHandler::from_env().unwrap();
// let tools = handler.get_tools(true); // Include Google Search
// let ai_service = AiService::with_function_handler(..., Box::new(handler));
// ```

pub mod google_docs_client;

#[allow(unused_imports)]
pub use google_docs_client::{
    read_google_doc_function, GoogleDocsClient, GoogleDocsFunctionHandler, ProjectDocsConfig,
};
