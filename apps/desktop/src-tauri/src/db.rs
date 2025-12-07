use std::sync::{Arc, Mutex};
use sqlx::{sqlite::SqlitePool, Row};
use llm_gateway::Message;
use agent_core::HistoryRepository;
use anyhow::Result;
use async_trait::async_trait;

pub struct SqliteHistory {
    pool: SqlitePool,
}

impl SqliteHistory {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl HistoryRepository for SqliteHistory {
    async fn add_message(&self, session_id: &str, message: Message) -> Result<()> {
        let sid = session_id.to_string();
        sqlx::query("INSERT INTO messages (session_id, role, content) VALUES ($1, $2, $3)")
            .bind(sid)
            .bind(message.role)
            .bind(message.content)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_history(&self, session_id: &str) -> Result<Vec<Message>> {
        let sid = session_id.to_string();
        let rows = sqlx::query("SELECT role, content FROM messages WHERE session_id = $1 ORDER BY id ASC")
            .bind(sid)
            .fetch_all(&self.pool)
            .await?;

        let messages = rows.into_iter().map(|row| {
            Message {
                role: row.get("role"),
                content: row.get("content"),
            }
        }).collect();

        Ok(messages)
    }
}
