use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use futures::Stream;
use futures::StreamExt;
use reqwest::Client;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMRequest {
    pub messages: Vec<Message>,
    pub config: LLMConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum StreamEvent {
    Token(String),
    ToolStart(String), // tool name
    ToolArg(String, String), // key, value chunk
    ToolEnd,
    Error(String),
    Done,
}

#[derive(Deserialize)]
struct OpenAIStreamChunk {
    choices: Vec<OpenAIStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
}

// State Machine for XML Parsing
enum ParserState {
    Text,
    InTag(String), // Buffer accumulating tag name/attrs
    InToolArg(String), // Arg name
}

pub struct Parser {
    buffer: String,
    state: ParserState,
    current_tool: Option<String>,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            state: ParserState::Text,
            current_tool: None,
        }
    }

    pub fn process_chunk(&mut self, chunk: &str) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        self.buffer.push_str(chunk);

        loop {
            match &self.state {
                ParserState::Text => {
                    if let Some(idx) = self.buffer.find("<tool_code>") {
                        if idx > 0 {
                            events.push(StreamEvent::Token(self.buffer[..idx].to_string()));
                        }
                        self.buffer = self.buffer[idx + 11..].to_string();
                        self.state = ParserState::InTag("".to_string());
                    } else {
                        let partial_tag = self.buffer.rfind('<');
                        if let Some(p) = partial_tag {
                             if p < self.buffer.len() {
                                 let safe_text = self.buffer[..p].to_string();
                                 if !safe_text.is_empty() {
                                     events.push(StreamEvent::Token(safe_text));
                                     self.buffer = self.buffer[p..].to_string();
                                 }
                                 break;
                             }
                        }
                        if !self.buffer.is_empty() {
                            events.push(StreamEvent::Token(self.buffer.clone()));
                            self.buffer.clear();
                        }
                        break;
                    }
                },
                ParserState::InTag(_) => {
                    if let Some(end_idx) = self.buffer.find("</tool_code>") {
                         self.buffer = self.buffer[end_idx + 12..].to_string();
                         self.state = ParserState::Text;
                         continue;
                    }

                    if let Some(tool_start) = self.buffer.find("<tool") {
                        if let Some(tag_close) = self.buffer[tool_start..].find('>') {
                             let tag_content = &self.buffer[tool_start..tool_start+tag_close+1];
                             let name_attr = "name=\"";
                             if let Some(n_idx) = tag_content.find(name_attr) {
                                 if let Some(q_idx) = tag_content[n_idx+name_attr.len()..].find('"') {
                                     let name = &tag_content[n_idx+name_attr.len()..n_idx+name_attr.len()+q_idx];
                                     events.push(StreamEvent::ToolStart(name.to_string()));
                                     self.current_tool = Some(name.to_string());

                                     self.buffer = self.buffer[tool_start+tag_close+1..].to_string();
                                     self.state = ParserState::InToolArg("".to_string());
                                     continue;
                                 }
                             }
                        }
                    }
                    break;
                },
                ParserState::InToolArg(_) => {
                    if let Some(tool_end) = self.buffer.find("</tool>") {
                         events.push(StreamEvent::ToolEnd);
                         self.current_tool = None;
                         self.buffer = self.buffer[tool_end+7..].to_string();
                         self.state = ParserState::InTag("".to_string());
                         continue;
                    }

                    if let Some(start_tag_idx) = self.buffer.find('<') {
                         if let Some(end_tag_idx) = self.buffer[start_tag_idx..].find('>') {
                              let tag_full = &self.buffer[start_tag_idx..start_tag_idx+end_tag_idx+1];
                              if !tag_full.starts_with("</") {
                                  let arg_name = tag_full.trim_matches(|c| c == '<' || c == '>');
                                  let closing_tag = format!("</{}>", arg_name);

                                  if let Some(closing_idx) = self.buffer.find(&closing_tag) {
                                      let val = &self.buffer[start_tag_idx+end_tag_idx+1..closing_idx];
                                      events.push(StreamEvent::ToolArg(arg_name.to_string(), val.to_string()));
                                      self.buffer = self.buffer[closing_idx+closing_tag.len()..].to_string();
                                      continue;
                                  }
                              }
                         }
                    }
                    break;
                }
            }
        }
        events
    }
}

pub fn stream_chat(req: LLMRequest) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
    Box::pin(async_stream::stream! {
        if req.config.base_url.contains("mock") {
            let mock_text = "Checking filesystem... \n<tool_code><tool name=\"run_command\"><program>ls</program><args>-la</args></tool></tool_code>";
            let chunk_size = 5;
            let mut parser = Parser::new();
            for chunk in mock_text.chars().collect::<Vec<char>>().chunks(chunk_size) {
                 let s: String = chunk.iter().collect();
                 tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                 let events = parser.process_chunk(&s);
                 for event in events { yield event; }
            }
             let events = parser.process_chunk("");
             for event in events { yield event; }
             yield StreamEvent::Done;
             return;
        }

        let client = Client::new();
        let url = format!("{}/chat/completions", req.config.base_url.trim_end_matches('/'));

        let body = serde_json::json!({
            "model": req.config.model,
            "messages": req.messages,
            "temperature": req.config.temperature,
            "stream": true
        });

        let mut res = match client.post(&url)
            .header("Authorization", format!("Bearer {}", req.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await {
                Ok(r) => r,
                Err(e) => { yield StreamEvent::Error(e.to_string()); return; }
            };

        if !res.status().is_success() {
             yield StreamEvent::Error(format!("API Error: {}", res.status()));
             return;
        }

        let mut parser = Parser::new();
        while let Some(chunk_res) = res.chunk().await.transpose() {
             match chunk_res {
                 Ok(chunk) => {
                     let s = String::from_utf8_lossy(&chunk);
                     for line in s.lines() {
                         if line.starts_with("data: ") {
                             let json_str = &line[6..];
                             if json_str == "[DONE]" { yield StreamEvent::Done; return; }
                             if let Ok(data) = serde_json::from_str::<OpenAIStreamChunk>(json_str) {
                                 if let Some(choice) = data.choices.first() {
                                     if let Some(content) = &choice.delta.content {
                                         let events = parser.process_chunk(content);
                                         for event in events { yield event; }
                                     }
                                 }
                             }
                         }
                     }
                 },
                 Err(e) => { yield StreamEvent::Error(e.to_string()); }
             }
        }
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LLMResponse {
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<HashMap<String, u32>>,
}

pub async fn send_chat_logic(req: LLMRequest) -> Result<LLMResponse, String> {
    if req.config.base_url.contains("mock") {
             let content = "Checking filesystem... \n<tool_code><tool name=\"run_command\"><program>ls</program><args>-la</args></tool></tool_code>".to_string();
             let mut parser = Parser::new();
             let events = parser.process_chunk(&content);
             let mut tools = Vec::new();
             let mut current_tool_name = None;
             let mut current_args = HashMap::new();

             for e in events {
                 match e {
                     StreamEvent::ToolStart(n) => { current_tool_name = Some(n); current_args.clear(); }
                     StreamEvent::ToolArg(k, v) => { current_args.insert(k, v); }
                     StreamEvent::ToolEnd => {
                         if let Some(n) = current_tool_name.take() {
                             tools.push(ToolCall { name: n, arguments: current_args.clone() });
                         }
                     }
                     _ => {}
                 }
             }

             return Ok(LLMResponse {
                 role: "assistant".to_string(),
                 content,
                 tool_calls: Some(tools),
                 usage: None,
             });
    }

    let client = Client::new();
    let url = format!("{}/chat/completions", req.config.base_url.trim_end_matches('/'));
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
            return Err(format!("API Error: {}", res.status()));
    }

    #[derive(Deserialize)]
    struct LocalOpenAIResponse {
        choices: Vec<LocalOpenAIChoice>,
        #[serde(default)]
        usage: Option<HashMap<String, u32>>,
    }
    #[derive(Deserialize)]
    struct LocalOpenAIChoice {
        message: Message,
    }

    let open_ai_res: LocalOpenAIResponse = res.json().await.map_err(|e| e.to_string())?;

    let (role, content) = open_ai_res.choices.first()
        .map(|c| (c.message.role.clone(), c.message.content.clone()))
        .unwrap_or(("assistant".to_string(), "".to_string()));

    let mut parser = Parser::new();
    let events = parser.process_chunk(&content);
    let mut tools = Vec::new();
    let mut current_tool_name = None;
    let mut current_args = HashMap::new();

    for e in events {
            match e {
                StreamEvent::ToolStart(n) => { current_tool_name = Some(n); current_args.clear(); }
                StreamEvent::ToolArg(k, v) => { current_args.insert(k, v); }
                StreamEvent::ToolEnd => {
                    if let Some(n) = current_tool_name.take() {
                        tools.push(ToolCall { name: n, arguments: current_args.clone() });
                    }
                }
                _ => {}
            }
    }

    Ok(LLMResponse {
        role,
        content,
        tool_calls: Some(tools),
        usage: open_ai_res.usage,
    })
}
