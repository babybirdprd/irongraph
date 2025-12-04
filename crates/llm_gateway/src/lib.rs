use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

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

// The External "Graph" format (Sent to Frontend via Specta)
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: HashMap<String, String>,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct LLMResponse {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
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

fn parse_xml_tools(content: &str) -> Option<Vec<ToolCall>> {
    let start_tag = "<tool_code>";
    let end_tag = "</tool_code>";

    let start_idx = content.find(start_tag)?;
    let end_idx = content.find(end_tag)?;

    if start_idx >= end_idx {
        return None;
    }

    let xml_block = &content[start_idx..end_idx + end_tag.len()];
    let mut reader = Reader::from_str(xml_block);
    reader.trim_text(true);

    let mut tools = Vec::new();
    let mut current_tool: Option<ToolCall> = None;
    let mut current_arg_name: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name_bytes = e.name();
                let name_str = String::from_utf8_lossy(name_bytes.as_ref());

                if name_str == "tool_code" {
                    continue;
                } else if name_str == "tool" {
                     let mut tool_name = String::new();
                     for attr in e.attributes() {
                         if let Ok(attr) = attr {
                             if attr.key.as_ref() == b"name" {
                                 tool_name = String::from_utf8_lossy(&attr.value).to_string();
                             }
                         }
                     }
                     current_tool = Some(ToolCall {
                         name: tool_name,
                         arguments: HashMap::new(),
                     });
                } else {
                    // Start of an argument tag
                    if current_tool.is_some() {
                        current_arg_name = Some(name_str.to_string());
                    }
                }
            },
            Ok(Event::Text(e)) => {
                if let (Some(tool), Some(arg_name)) = (&mut current_tool, &current_arg_name) {
                     if let Ok(text) = e.unescape() {
                         tool.arguments.insert(arg_name.clone(), text.into_owned());
                     }
                }
            },
            Ok(Event::End(e)) => {
                let name_bytes = e.name();
                let name_str = String::from_utf8_lossy(name_bytes.as_ref());

                if name_str == "tool" {
                    if let Some(tool) = current_tool.take() {
                        tools.push(tool);
                    }
                } else if name_str == "tool_code" {
                    break;
                } else {
                     // End of argument
                     if current_arg_name.as_ref() == Some(&name_str.to_string()) {
                         current_arg_name = None;
                     }
                }
            },
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => (),
        }
    }

    if tools.is_empty() {
        // It's possible the block was empty or we failed to parse tools.
        // But if we found tool_code tags, we probably want to return Some([]) instead of None if it was just empty?
        // Logic says return None if *parsing fails*.
        // If <tool_code></tool_code> is present but empty, is it valid?
        // The implementation assumes if we parse successfully we return Some(vec).
        // If valid empty block, we return Some(empty).
        // But here I'll just return Some(tools).
        Some(tools)
    } else {
        Some(tools)
    }
}

pub mod commands {
    use super::*;

    #[tauri::command]
    #[specta::specta]
    pub async fn send_chat(req: LLMRequest) -> Result<LLMResponse, String> {
        // 1. Mock Mode
        if req.config.base_url.contains("mock") {
            let content = "Checking filesystem... \n<tool_code><tool name=\"run_command\"><program>ls</program><args>-la</args></tool></tool_code>".to_string();
            let tool_calls = parse_xml_tools(&content);
            return Ok(LLMResponse {
                role: "assistant".to_string(),
                content,
                tool_calls,
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

        let (role, content) = open_ai_res.choices.first()
            .map(|c| (c.message.role.clone(), c.message.content.clone()))
            .unwrap_or(("assistant".to_string(), "".to_string()));

        let tool_calls = parse_xml_tools(&content);

        Ok(LLMResponse {
            role,
            content,
            tool_calls,
            usage: open_ai_res.usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::commands::send_chat;

    #[test]
    fn test_parse_xml_tools() {
        let content = r#"
            Here is the plan:
            <tool_code>
                <tool name="run_command">
                    <program>ls</program>
                    <args>-la</args>
                </tool>
            </tool_code>
        "#;
        let tools = parse_xml_tools(content).expect("Should parse");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "run_command");
        assert_eq!(tools[0].arguments.get("program").unwrap(), "ls");
        assert_eq!(tools[0].arguments.get("args").unwrap(), "-la");
    }

    #[test]
    fn test_parse_xml_multiple_tools() {
        let content = r#"
            I will run two commands:
            <tool_code>
                <tool name="cmd1">
                    <arg>val1</arg>
                </tool>
                <tool name="cmd2">
                    <arg>val2</arg>
                </tool>
            </tool_code>
        "#;
        let tools = parse_xml_tools(content).expect("Should parse multiple");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "cmd1");
        assert_eq!(tools[1].name, "cmd2");
    }

    #[test]
    fn test_parse_xml_no_tools() {
        let content = "Just some text without tools.";
        assert!(parse_xml_tools(content).is_none());
    }

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
        assert!(res.content.contains("Checking filesystem"));
        assert!(res.tool_calls.is_some());
        let tools = res.tool_calls.unwrap();
        assert_eq!(tools[0].name, "run_command");
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
                            "content": "Hello there! <tool_code><tool name=\"greet\"><msg>hi</msg></tool></tool_code>"
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
        assert!(res.content.contains("Hello there!"));
        assert!(res.tool_calls.is_some());
        assert_eq!(res.tool_calls.unwrap()[0].name, "greet");
    }
}
