pub mod ai_service;
pub mod context;
pub mod formatting;
pub mod knowledge;
pub mod models;

pub use ai_service::{AiProvider, AiService, FunctionCallHandler};
#[allow(unused_imports)]
pub use context::{select_context, ContextMessage, ContextSelector};
pub use formatting::format_citations_for_discord;
#[allow(unused_imports)]
pub use knowledge::{KnowledgeChunk, KnowledgeStore};
#[allow(unused_imports)]
pub use models::{
    AiConfig, AiMessage, AiProviderResponse, AiResponseWithMeta, AiTool, Citation, FunctionCall,
    FunctionDef,
};
