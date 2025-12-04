use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct LLMConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct LLMRequest {
    pub messages: Vec<Message>,
    pub config: LLMConfig,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct LLMResponse {
    pub content: String,
    pub usage: Option<HashMap<String, u32>>,
}

// Internal OpenAI API Response Structures (Private)
#[derive(Deserialize)]
struct OpenAIChoice {
    message: Message,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<HashMap<String, u32>>,
}

pub mod commands {
    use super::*;

    #[tauri::command]
    #[specta::specta]
    pub async fn send_chat(req: LLMRequest) -> Result<LLMResponse, String> {
        // 1. Mock Mode
        if req.config.base_url.contains("mock") {
            return Ok(LLMResponse {
                content: "IronGraph Mock: System is online. Connection successful.".to_string(),
                usage: None,
            });
        }

        // 2. Real Request
        let client = reqwest::Client::new();

        let url = format!("{}/chat/completions", req.config.base_url.trim_end_matches('/'));

        // OpenAI format expects "model" and "messages" at root
        let body = serde_json::json!({
            "model": req.config.model,
            "messages": req.messages,
            "temperature": req.config.temperature
        });

        let res = client.post(&url)
            .header("Authorization", format!("Bearer {}", req.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("API Error {}: {}", status, text));
        }

        let open_ai_res: OpenAIResponse = res.json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let content = open_ai_res.choices.first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(LLMResponse {
            content,
            usage: open_ai_res.usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::commands::send_chat;

    #[tokio::test]
    async fn test_mock_mode() {
        let req = LLMRequest {
            messages: vec![Message { role: "user".into(), content: "hi".into() }],
            config: LLMConfig {
                api_key: "dummy".into(),
                base_url: "mock".into(),
                model: "gpt-4o".into(),
                temperature: 0.7,
            },
        };

        let res = send_chat(req).await.expect("Mock should succeed");
        assert_eq!(res.content, "IronGraph Mock: System is online. Connection successful.");
    }

    #[tokio::test]
    async fn test_real_api_structure() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let mock = server.mock("POST", "/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"
                {
                    "id": "chatcmpl-123",
                    "object": "chat.completion",
                    "created": 1677652288,
                    "model": "gpt-3.5-turbo-0613",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "Hello there!"
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 9,
                        "completion_tokens": 12,
                        "total_tokens": 21
                    }
                }
            "#)
            .create_async().await;

        let req = LLMRequest {
            messages: vec![Message { role: "user".into(), content: "Hello".into() }],
            config: LLMConfig {
                api_key: "sk-test".into(),
                base_url: url.clone(),
                model: "gpt-3.5-turbo".into(),
                temperature: 0.5,
            },
        };

        let res = send_chat(req).await.expect("Request should succeed");

        mock.assert_async().await;
        assert_eq!(res.content, "Hello there!");
        assert!(res.usage.is_some());
    }
}
