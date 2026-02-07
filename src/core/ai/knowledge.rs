use async_trait::async_trait;
use std::error::Error;

/// A chunk of knowledge that can be stored and retrieved for RAG
#[derive(Debug, Clone)]
pub struct KnowledgeChunk {
    pub id: i64,
    pub category: String,
    pub content: String,
    pub keywords: Vec<String>,
    pub created_at: i64,
}

/// Trait for storing and retrieving knowledge chunks
#[async_trait]
pub trait KnowledgeStore: Send + Sync {
    /// Add a new knowledge chunk and return its ID
    async fn add_chunk(
        &self,
        category: &str,
        content: &str,
        keywords: &[&str],
    ) -> Result<i64, Box<dyn Error + Send + Sync>>;

    /// Search for knowledge chunks by query string (matches against content and keywords)
    async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeChunk>, Box<dyn Error + Send + Sync>>;

    /// Get knowledge chunks by category
    async fn get_by_category(
        &self,
        category: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeChunk>, Box<dyn Error + Send + Sync>>;

    /// Delete a knowledge chunk by ID
    async fn delete_chunk(&self, id: i64) -> Result<(), Box<dyn Error + Send + Sync>>;
}
