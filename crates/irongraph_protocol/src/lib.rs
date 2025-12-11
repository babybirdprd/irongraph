use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

// ==========================================
// Workspace Manager Protocols
// ==========================================

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileEntry>>,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct FileContent {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize, Type)]
pub enum FsError {
    Io(String),
    SecurityViolation,
    InvalidPath,
    Syntax(String),
}

// ==========================================
// Terminal Manager Protocols
// ==========================================

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Serialize, Type)]
pub enum ShellError {
    Io(String),
    NotFound(String),
    Pty(String),
}

// ==========================================
// Feature Profile Protocols
// ==========================================

#[derive(Type, Serialize, Deserialize, Debug)]
pub struct UpdateProfileReq {
    pub name: String,
    pub bio: String,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct UserProfile {
    pub id: i32,
    pub name: String,
    pub bio: String,
}

// ==========================================
// LLM Gateway Protocols
// ==========================================

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
