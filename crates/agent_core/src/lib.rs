use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Window, Emitter};
use tokio::sync::mpsc;
use async_trait::async_trait;
use radkit::models::providers::OpenRouterLlm;
use radkit::models::{BaseLlm, ContentPart, Thread, Event};
use radkit::tools::{BaseToolset, SimpleToolset, ToolContext, ToolResponse};

// Imports for tools
use workspace_manager::tools::{read_file, write_file, list_files, read_skeleton, search_code};
use terminal_manager::tools::{run_command};
use common::{RadkitState, TerminalState, SessionState, register_session, unregister_session};

mod shell;

// Define HistoryRepository trait for persistence abstraction
#[async_trait]
pub trait HistoryRepository: Send + Sync {
    async fn add_message(&self, session_id: &str, message: serde_json::Value) -> anyhow::Result<()>;
    async fn get_history(&self, session_id: &str) -> anyhow::Result<Vec<serde_json::Value>>;
}

pub struct AgentSession {
    pub id: String,
    pub repository: Arc<Box<dyn HistoryRepository>>,
    pub status: AtomicBool,
    pub terminal_session_id: Mutex<Option<String>>,
    pub command_buffer: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    pub terminal_state: Option<Arc<TerminalState>>,
}

impl AgentSession {
    pub fn new(repository: Box<dyn HistoryRepository>, terminal_state: Arc<TerminalState>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            repository: Arc::new(repository),
            status: AtomicBool::new(false),
            terminal_session_id: Mutex::new(None),
            command_buffer: Arc::new(Mutex::new(None)),
            terminal_state: Some(terminal_state),
        }
    }
}

impl Drop for AgentSession {
    fn drop(&mut self) {
        unregister_session(&self.id);
        if let Some(state) = &self.terminal_state {
            if let Ok(guard) = self.terminal_session_id.lock() {
                if let Some(id) = guard.as_ref() {
                    let _ = terminal_manager::kill_session(state, id);
                }
            }
        }
    }
}

const SYSTEM_PROMPT: &str = "You are IronGraph, an advanced AI software engineer. You are running in a Tauri environment.";

// Config struct to allow passing API key
#[derive(serde::Deserialize, Clone)]
pub struct LLMConfig {
    pub api_key: String,
    pub model: String,
}

pub async fn spawn_agent_loop(
    window: Window,
    session: Arc<AgentSession>,
    workspace_state: Arc<Mutex<std::path::PathBuf>>,
    terminal_state: Arc<TerminalState>,
    initial_prompt: String,
    config: LLMConfig,
) {
    let session_id = session.id.clone();
    let session_clone = session.clone();

    // 1. Ensure Terminal Session Exists
    {
        let mut ts_lock = session.terminal_session_id.lock().unwrap();
        if ts_lock.is_none() {
            let root = workspace_state.lock().unwrap().clone();
            let (tx, mut rx) = mpsc::channel(100);

            match terminal_manager::start_terminal_session(&root, &terminal_state, tx) {
                Ok(tid) => {
                    *ts_lock = Some(tid.clone());
                    let win_clone = window.clone();
                    let buffer_arc = session_clone.command_buffer.clone();

                    tokio::spawn(async move {
                         while let Some(out) = rx.recv().await {
                             let _ = win_clone.emit(&format!("agent:terminal:output:{}", tid), out.clone());

                             let sender_opt = {
                                 buffer_arc.lock().unwrap().clone()
                             };
                             if let Some(sender) = sender_opt {
                                 let _ = sender.send(out).await;
                             }
                         }
                    });
                }
                Err(e) => {
                    println!("Failed to start terminal session: {}", e);
                    let _ = window.emit(&format!("agent:status:{}", session_id), "error");
                    return;
                }
            }
        }
    }

    session.status.store(true, Ordering::Relaxed);
    let _ = window.emit(&format!("agent:status:{}", session_id), "running");

    let root_path = workspace_state.lock().unwrap().clone();
    let terminal_sid = session.terminal_session_id.lock().unwrap().clone().unwrap();

    // Register Heavy State
    let agent_state = Arc::new(RadkitState {
        root: root_path.clone(),
        terminal_state: terminal_state.clone(),
        session_id: terminal_sid,
        command_buffer: session.command_buffer.clone(),
    });
    register_session(session_id.clone(), agent_state);

    // Prepare Light State
    let light_state = SessionState::new(session_id.clone());

    // Use config
    let llm = OpenRouterLlm::new(config.model, config.api_key)
        .with_site_url("https://irongraph.app")
        .with_app_name("IronGraph");

    // Setup Tools
    use radkit::tools::BaseTool;
    let tools: Vec<Box<dyn BaseTool>> = vec![
        Box::new(read_file),
        Box::new(write_file),
        Box::new(list_files),
        Box::new(read_skeleton),
        Box::new(search_code),
        Box::new(run_command),
    ];
    let toolset = Arc::new(SimpleToolset::new(tools)) as Arc<dyn BaseToolset>;

    // Create ToolContext with our session state
    let tool_context = match ToolContext::builder().with_state(&light_state).build() {
        Ok(ctx) => ctx,
        Err(e) => {
            let _ = window.emit(&format!("agent:error:{}", session_id), format!("Context Init Failed: {}", e));
            return;
        }
    };

    // Load History
    let mut thread = Thread::from_system(SYSTEM_PROMPT);

    // Load from DB
    if let Ok(history) = session.repository.get_history(&session_id).await {
        for msg in history {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");

            // Note: radkit 0.0.3 manual Thread construction from JSON is limited.
            // Simplified: Add text messages
            if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                if role == "user" {
                    thread = thread.add_event(Event::user(content));
                } else if role == "assistant" {
                    thread = thread.add_event(Event::assistant(content));
                }
            }
        }
    }

    // Add Current User Prompt
    thread = thread.add_event(Event::user(initial_prompt.clone()));

    // Persist Initial User Message
    let user_msg_json = serde_json::json!({
        "role": "user",
        "content": initial_prompt
    });
    let _ = session.repository.add_message(&session_id, user_msg_json).await;

    let max_iterations = 20;
    let mut iterations = 0;

    loop {
        if !session.status.load(Ordering::Relaxed) {
            break;
        }

        iterations += 1;
        if iterations > max_iterations {
            let _ = window.emit(&format!("agent:error:{}", session_id), "Max iterations reached");
            break;
        }

        match llm.generate_content(thread.clone(), Some(toolset.clone())).await {
            Ok(response) => {
                let content = response.into_content();

                // Add Assistant Message to Thread
                thread = thread.add_event(Event::assistant(content.clone()));

                // Process Content Parts
                let mut tool_calls = Vec::new();
                let mut text_content = String::new();

                for part in content.parts() {
                    match part {
                        ContentPart::Text(t) => {
                            text_content.push_str(t);
                            let _ = window.emit(&format!("agent:token:{}", session_id), t);
                        },
                        ContentPart::ToolCall(call) => {
                            tool_calls.push(call.clone());
                            let _ = window.emit(&format!("agent:tool_start:{}", session_id), call.name());

                            // Persist tool call
                            let msg = serde_json::json!({
                                "role": "assistant",
                                "tool_calls": [{
                                    "id": call.id(),
                                    "type": "function",
                                    "function": {
                                        "name": call.name(),
                                        "arguments": call.arguments().to_string()
                                    }
                                }]
                            });
                            let _ = session.repository.add_message(&session_id, msg).await;
                        },
                        _ => {}
                    }
                }

                if !text_content.is_empty() {
                     let msg = serde_json::json!({
                        "role": "assistant",
                        "content": text_content
                    });
                    let _ = session.repository.add_message(&session_id, msg).await;
                }

                if tool_calls.is_empty() {
                    // Done
                    let _ = window.emit(&format!("agent:status:{}", session_id), "waiting");
                    session.status.store(false, Ordering::Relaxed);
                    break;
                }

                // Execute Tools
                let tools_map = toolset.get_tools().await; // Returns Vec<&dyn BaseTool>

                for call in tool_calls {
                    // Find tool
                    if let Some(tool) = tools_map.iter().find(|t| t.name() == call.name()) {
                        let args_res = call.arguments().as_object().ok_or("Args not object");
                        if let Some(args_map) = args_res.ok().map(|m| m.iter().map(|(k,v)| (k.clone(), v.clone())).collect()) {
                             let result = tool.run_async(args_map, &tool_context).await;

                             let output_display = format!("Tool Output:\n{}", result.data());
                             let _ = window.emit(&format!("agent:tool_output:{}", session_id), output_display);

                             let response = ToolResponse::new(call.id().to_string(), result);

                             // Add Tool Response to Thread
                             thread = thread.add_event(Event::from(response.clone()));

                             // Persist result
                             let msg = serde_json::json!({
                                "role": "tool",
                                "tool_call_id": call.id(),
                                "content": response.result().data().to_string()
                             });
                             let _ = session.repository.add_message(&session_id, msg).await;

                        } else {
                             // Arg parse error
                             let _ = window.emit(&format!("agent:error:{}", session_id), "Tool args parse error");
                        }
                    } else {
                        // Tool not found
                        let _ = window.emit(&format!("agent:error:{}", session_id), format!("Tool not found: {}", call.name()));
                    }
                }
            }
            Err(e) => {
                println!("LLM Error: {}", e);
                let _ = window.emit(&format!("agent:error:{}", session_id), e.to_string());
                break;
            }
        }
    }
}
