use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Window, Emitter};
use tokio::sync::mpsc;
use async_trait::async_trait;
use radkit::models::providers::OpenRouterLlm;
use radkit::models::{BaseLlm, ContentPart, Thread, Event};
use radkit::tools::{BaseToolset, SimpleToolset, ToolContext, ToolResponse};
use serde::{Deserialize, Serialize};

// Imports for tools
use workspace_manager::tools::{read_file, write_file, list_files, read_skeleton, search_code};
use terminal_manager::tools::{run_command};
use common::{RadkitState, TerminalState, SessionState, register_session, unregister_session};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentRole {
    Coder,
    Verifier,
}

impl AgentRole {
    fn as_str(&self) -> &'static str {
        match self {
            AgentRole::Coder => "coder",
            AgentRole::Verifier => "verifier",
        }
    }
}

const CODER_PROMPT: &str = r#"You are the Architect (Coder).
Your goal is to implement the requested solution efficiently and correctly.
You have access to tools to write code, read files, and explore the project.
Do NOT run tests yourself. Just focus on writing the best possible implementation.
Once you have written the code, the Verifier will take over to test it."#;

const VERIFIER_PROMPT: &str = r#"You are the Adversary (Verifier).
Your goal is to PROVE the Coder's implementation is flawed.
Trust nothing.
1. Analyze the code just written.
2. Write a reproduction script or test case (e.g., test_repro.rs) that targets edge cases or potential bugs.
3. Run the test using `run_command`.
   - If the test FAILS (Exit Code != 0), you have succeeded. The Coder will be summoned to fix it.
   - If the test PASSES (Exit Code 0), you have failed to break it.
4. If you cannot break the code and are satisfied it is correct, output the exact tag: <verified />"#;

fn get_prompt_for_role(role: &AgentRole) -> &'static str {
    match role {
        AgentRole::Coder => CODER_PROMPT,
        AgentRole::Verifier => VERIFIER_PROMPT,
    }
}

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

    // Initialize State Machine
    let mut current_role = AgentRole::Coder;
    let mut verification_attempts = 0;
    const MAX_VERIFICATION_ATTEMPTS: i32 = 5;

    // Load History
    let mut thread = Thread::from_system(get_prompt_for_role(&current_role));

    // Load from DB
    if let Ok(history) = session.repository.get_history(&session_id).await {
        for msg in history {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");

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
        "content": initial_prompt,
        "metadata": { "persona": "user" }
    });
    let _ = session.repository.add_message(&session_id, user_msg_json).await;

    let max_iterations = 40; // Increased for dual loop
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
                let mut role_transition = None;

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
                                }],
                                "metadata": { "persona": current_role.as_str() }
                            });
                            let _ = session.repository.add_message(&session_id, msg).await;
                        },
                        _ => {}
                    }
                }

                if !text_content.is_empty() {
                     let msg = serde_json::json!({
                        "role": "assistant",
                        "content": text_content,
                        "metadata": { "persona": current_role.as_str() }
                    });
                    let _ = session.repository.add_message(&session_id, msg).await;

                    // Check for termination from Verifier
                    if current_role == AgentRole::Verifier && text_content.contains("<verified />") {
                        let _ = window.emit(&format!("agent:status:{}", session_id), "waiting");
                        session.status.store(false, Ordering::Relaxed);
                        break;
                    }
                }

                if tool_calls.is_empty() {
                    // No tools called.
                    // If Verifier didn't verify, it might be waiting or just chatting.
                    // Usually we wait for user input here, or if Verifier is stuck we might need to nudge.
                    // For now, assume it waits for user.

                    // If Coder returns just text, maybe it's done or asking clarification.
                    // We just break loop and wait for user.
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
                             let output_data = result.data().to_string();

                             let output_display = format!("Tool Output:\n{}", output_data);
                             let _ = window.emit(&format!("agent:tool_output:{}", session_id), output_display);

                             let response = ToolResponse::new(call.id().to_string(), result);

                             // Add Tool Response to Thread
                             thread = thread.add_event(Event::from(response.clone()));

                             // Persist result
                             let msg = serde_json::json!({
                                "role": "tool",
                                "tool_call_id": call.id(),
                                "content": output_data.clone(),
                                "metadata": { "persona": current_role.as_str() }
                             });
                             let _ = session.repository.add_message(&session_id, msg).await;

                             // --- STATE MACHINE LOGIC ---
                             match current_role {
                                 AgentRole::Coder => {
                                     // Transition Coder -> Verifier on 'write_file'
                                     if call.name() == "write_file" {
                                         role_transition = Some(AgentRole::Verifier);
                                     }
                                 },
                                 AgentRole::Verifier => {
                                     // Check for 'run_command' results
                                     if call.name() == "run_command" {
                                         // Check exit code
                                         if output_data.contains("(Exit Code: 0)") {
                                             // Passed.
                                             // Verifier should see this and output <verified /> next turn.
                                         } else {
                                             // Failed (Exit Code != 0).
                                             // Verifier succeeded in breaking it. Back to Coder.
                                             role_transition = Some(AgentRole::Coder);
                                         }
                                     }
                                 }
                             }

                        } else {
                             // Arg parse error
                             let _ = window.emit(&format!("agent:error:{}", session_id), "Tool args parse error");
                        }
                    } else {
                        // Tool not found
                        let _ = window.emit(&format!("agent:error:{}", session_id), format!("Tool not found: {}", call.name()));
                    }
                }

                // Handle Transitions
                if let Some(new_role) = role_transition {
                    if new_role != current_role {
                        if new_role == AgentRole::Verifier {
                            // Coder -> Verifier
                             verification_attempts += 1;
                             if verification_attempts > MAX_VERIFICATION_ATTEMPTS {
                                 let _ = window.emit(&format!("agent:error:{}", session_id), "Max verification attempts reached. Aborting.");
                                 session.status.store(false, Ordering::Relaxed);
                                 break;
                             }
                        }

                        current_role = new_role;
                        let prompt = get_prompt_for_role(&current_role);
                        // Inject System Prompt for new role
                        // Radkit Thread is immutable, so we add a system message event if supported or simulate it
                        // Since `Event::system` might not be exposed or standard in this version of radkit,
                        // we can simulate it with a User message instructing the role change,
                        // OR if radkit supports system events mid-stream (some LLMs do).
                        // However, radkit `Thread` usually starts with system.
                        // Let's add a User message that ACTS as a system instruction to enforce the role.

                        let role_msg = format!("\n[SYSTEM]: SWITCHING ROLE.\n{}", prompt);
                        thread = thread.add_event(Event::user(role_msg.clone()));

                        println!("[Agent Loop] Switching Role to: {}", current_role.as_str());

                        // Notify Frontend of role change (optional, helpful for debug)
                        let _ = window.emit(&format!("agent:debug:role:{}", session_id), current_role.as_str());
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
