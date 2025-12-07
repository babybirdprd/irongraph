use sqlx::{sqlite::SqlitePool, Row};
use agent_core::HistoryRepository;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

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
    async fn add_message(&self, session_id: &str, message: Value) -> Result<()> {
        let sid = session_id.to_string();
        let role = message.get("role").and_then(|v| v.as_str()).unwrap_or("user").to_string();

        let content_to_store = if let Some(content_str) = message.get("content").and_then(|v| v.as_str()) {
            content_str.to_string()
        } else if message.get("tool_calls").is_some() {
            // Store the whole JSON for tool calls
            message.to_string()
        } else {
            message.to_string()
        };

        sqlx::query("INSERT INTO messages (session_id, role, content) VALUES ($1, $2, $3)")
            .bind(sid)
            .bind(role)
            .bind(content_to_store)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_history(&self, session_id: &str) -> Result<Vec<Value>> {
        let sid = session_id.to_string();
        let rows = sqlx::query("SELECT role, content FROM messages WHERE session_id = $1 ORDER BY id ASC")
            .bind(sid)
            .fetch_all(&self.pool)
            .await?;

        let messages = rows.into_iter().map(|row| {
            let role: String = row.get("role");
            let content: String = row.get("content");

            // Heuristic: If it starts with {, assume JSON
            if content.trim().starts_with('{') {
                if let Ok(val) = serde_json::from_str::<Value>(&content) {
                    return val;
                }
            }

            serde_json::json!({
                "role": role,
                "content": content
            })
        }).collect();

        Ok(messages)
    }
}
