use crate::core::ai::knowledge::{KnowledgeChunk, KnowledgeStore};
use async_trait::async_trait;
use sqlx::{Pool, Row, Sqlite};
use std::error::Error;

pub struct SqliteKnowledgeStore {
    pool: Pool<Sqlite>,
}

impl SqliteKnowledgeStore {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn migrate(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS knowledge_chunks (
                id INTEGER PRIMARY KEY,
                category TEXT NOT NULL,
                content TEXT NOT NULL,
                keywords TEXT NOT NULL,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create index for faster category lookups
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_knowledge_chunks_category 
            ON knowledge_chunks(category)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    fn serialize_keywords(keywords: &[&str]) -> String {
        serde_json::to_string(&keywords).unwrap_or_else(|_| "[]".to_string())
    }

    fn deserialize_keywords(keywords_json: &str) -> Vec<String> {
        serde_json::from_str(keywords_json).unwrap_or_default()
    }
}

#[async_trait]
impl KnowledgeStore for SqliteKnowledgeStore {
    async fn add_chunk(
        &self,
        category: &str,
        content: &str,
        keywords: &[&str],
    ) -> Result<i64, Box<dyn Error + Send + Sync>> {
        let keywords_json = Self::serialize_keywords(keywords);
        let created_at = chrono::Utc::now().timestamp();

        let result = sqlx::query(
            r#"
            INSERT INTO knowledge_chunks (category, content, keywords, created_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(category)
        .bind(content)
        .bind(keywords_json)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeChunk>, Box<dyn Error + Send + Sync>> {
        let search_pattern = format!("%{}%", query);

        let rows = sqlx::query(
            r#"
            SELECT id, category, content, keywords, created_at
            FROM knowledge_chunks
            WHERE content LIKE ? OR keywords LIKE ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(&search_pattern)
        .bind(&search_pattern)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let chunks = rows
            .into_iter()
            .map(|row| {
                let keywords_json: String = row.get("keywords");
                KnowledgeChunk {
                    id: row.get("id"),
                    category: row.get("category"),
                    content: row.get("content"),
                    keywords: Self::deserialize_keywords(&keywords_json),
                    created_at: row.get("created_at"),
                }
            })
            .collect();

        Ok(chunks)
    }

    async fn get_by_category(
        &self,
        category: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeChunk>, Box<dyn Error + Send + Sync>> {
        let rows = sqlx::query(
            r#"
            SELECT id, category, content, keywords, created_at
            FROM knowledge_chunks
            WHERE category = ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(category)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let chunks = rows
            .into_iter()
            .map(|row| {
                let keywords_json: String = row.get("keywords");
                KnowledgeChunk {
                    id: row.get("id"),
                    category: row.get("category"),
                    content: row.get("content"),
                    keywords: Self::deserialize_keywords(&keywords_json),
                    created_at: row.get("created_at"),
                }
            })
            .collect();

        Ok(chunks)
    }

    async fn delete_chunk(&self, id: i64) -> Result<(), Box<dyn Error + Send + Sync>> {
        sqlx::query("DELETE FROM knowledge_chunks WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
