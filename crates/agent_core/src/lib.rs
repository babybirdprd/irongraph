use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use tauri::{Window, Emitter};
use llm_gateway::{LLMRequest, Message, LLMConfig, StreamEvent, ToolCall, stream_chat};
use futures::StreamExt;
use terminal_manager::TerminalState;
use tokio::sync::mpsc;

pub struct AgentSession {
    pub id: String,
    pub history: Arc<Mutex<Vec<Message>>>,
    pub status: AtomicBool,
    pub terminal_session_id: Mutex<Option<String>>,
    // Buffer for active command output
    pub command_buffer: Arc<Mutex<Option<mpsc::Sender<String>>>>,
}

impl AgentSession {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            history: Arc::new(Mutex::new(Vec::new())),
            status: AtomicBool::new(false),
            terminal_session_id: Mutex::new(None),
            command_buffer: Arc::new(Mutex::new(None)),
        }
    }
}

const SYSTEM_PROMPT: &str = "You are IronGraph, an advanced AI software engineer...";

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

    {
        let mut history = session.history.lock().unwrap();
        if history.is_empty() {
             history.push(Message { role: "system".into(), content: SYSTEM_PROMPT.into() });
        }
        history.push(Message { role: "user".into(), content: initial_prompt });
    }

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
                             // 1. Emit to frontend
                             let _ = win_clone.emit(&format!("agent:terminal:output:{}", tid), out.clone());

                             // 2. Forward to active command buffer if present
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
                }
            }
        }
    }

    session.status.store(true, Ordering::Relaxed);

    tokio::spawn(async move {
        let _ = window.emit(&format!("agent:status:{}", session_id), "running");

        loop {
            if !session.status.load(Ordering::Relaxed) {
                break;
            }

            let messages = {
                session.history.lock().unwrap().clone()
            };

            let req = LLMRequest {
                messages,
                config: config.clone(),
            };

            let mut stream = stream_chat(req);

            let mut current_tool_name: Option<String> = None;
            let mut current_tool_args: HashMap<String, String> = HashMap::new();

            let mut assistant_content = String::new();
            let mut tool_calls = Vec::new();

            while let Some(event) = stream.next().await {
                if !session.status.load(Ordering::Relaxed) { break; }

                let _ = window.emit(&format!("agent:token:{}", session_id), event.clone());

                match event {
                    StreamEvent::Token(t) => assistant_content.push_str(&t),
                    StreamEvent::ToolStart(name) => {
                        current_tool_name = Some(name);
                        current_tool_args.clear();
                    },
                    StreamEvent::ToolArg(k, v) => {
                        current_tool_args.insert(k, v);
                    },
                    StreamEvent::ToolEnd => {
                        if let Some(name) = current_tool_name.take() {
                             tool_calls.push(ToolCall {
                                 name,
                                 arguments: current_tool_args.clone(),
                             });
                        }
                    },
                    StreamEvent::Error(e) => {
                        println!("Agent Stream Error: {}", e);
                    },
                    StreamEvent::Done => {}
                }
            }

            {
                let mut history = session.history.lock().unwrap();
                history.push(Message {
                    role: "assistant".into(),
                    content: assistant_content.clone(),
                });
            }

            if tool_calls.is_empty() {
                let _ = window.emit(&format!("agent:status:{}", session_id), "waiting");
                session.status.store(false, Ordering::Relaxed);
                break;
            }

            for tool in tool_calls {
                let output = execute_tool(&tool, &session, &workspace_state, &terminal_state).await;

                let result_msg = format!("Tool Output [{}]:\n{}", tool.name, output);

                {
                    let mut history = session.history.lock().unwrap();
                    history.push(Message {
                        role: "user".into(),
                        content: result_msg.clone(),
                    });
                }

                let _ = window.emit(&format!("agent:tool_output:{}", session_id), result_msg);
            }
        }
    });
}

async fn execute_tool(
    tool: &ToolCall,
    session: &Arc<AgentSession>,
    workspace_state_arc: &Arc<Mutex<std::path::PathBuf>>,
    terminal_state_arc: &Arc<TerminalState>
) -> String {
    let root = match workspace_state_arc.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return "Error: Workspace Lock Poisoned".to_string(),
    };

    match tool.name.as_str() {
        "run_command" => {
            let program = tool.arguments.get("program").cloned().unwrap_or_default();
            let args_str = tool.arguments.get("args").cloned().unwrap_or_default();
            let args = shlex::split(&args_str).unwrap_or_default();

            let tid_opt = session.terminal_session_id.lock().unwrap().clone();
            if let Some(tid) = tid_opt {
                 // 1. Setup buffer channel
                 let (tx, mut rx) = mpsc::channel(100);
                 {
                     let mut buf_lock = session.command_buffer.lock().unwrap();
                     *buf_lock = Some(tx);
                 }

                 // 2. Inject Sentinel
                 // We don't use terminal_manager::run_command_internal directly because it appends \n.
                 // We want to construct "cmd; echo ...".
                 // We will use terminal_manager::write_to_pty (via public API?)
                 // terminal_manager only exposes write_to_pty and run_command_internal.
                 // run_command_internal uses "program args".
                 // We want to inject shell syntax.
                 // If the persistent session is bash, we can run `program args; echo...`?
                 // `run_command_internal` joins args.
                 // We should manually construct the command line.
                 // We will bypass `run_command_internal` logic slightly or abuse it.
                 // Let's manually invoke write_to_pty.

                 let cmd_str = if args.is_empty() {
                     program
                 } else {
                     // Need to escape args? shlex::join is best but not available in std.
                     // Simple join.
                     format!("{} {}", program, args.join(" "))
                 };

                 let sentinel_cmd = format!("{}; echo \"IRONGRAPH_CMD_DONE:$?\"\n", cmd_str);

                 if let Err(e) = terminal_manager::write_to_pty(terminal_state_arc, &tid, &sentinel_cmd) {
                     return format!("Error writing to PTY: {}", e);
                 }

                 // 3. Accumulate Output
                 let mut output = String::new();
                 let start = std::time::Instant::now();
                 let timeout = std::time::Duration::from_secs(30); // Max wait

                 loop {
                     let chunk = match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                         Ok(Some(s)) => s,
                         Ok(None) => break, // Channel closed
                         Err(_) => {
                             // Timeout on READ (no output for 5s).
                             // Are we done? Maybe interactive prompt?
                             // Check total timeout
                             if start.elapsed() > timeout {
                                 output.push_str("\n[IronGraph: Timeout waiting for sentinel]");
                                 break;
                             }
                             continue;
                         }
                     };

                     output.push_str(&chunk);

                     if let Some(idx) = output.find("IRONGRAPH_CMD_DONE:") {
                         // Extract code?
                         // "IRONGRAPH_CMD_DONE:0\n"
                         // We strip everything from idx onwards for the return value?
                         let ret = output[..idx].to_string();
                         // We could parse exit code to append "Exit Code: 0"
                         let rest = &output[idx..];
                         let code = rest.trim_start_matches("IRONGRAPH_CMD_DONE:").trim();
                         // code might be "0", "1", etc.

                         // Clear buffer
                         {
                             let mut buf_lock = session.command_buffer.lock().unwrap();
                             *buf_lock = None;
                         }

                         return format!("{}\n(Exit Code: {})", ret.trim(), code);
                     }
                 }

                 {
                     let mut buf_lock = session.command_buffer.lock().unwrap();
                     *buf_lock = None;
                 }
                 output

            } else {
                "Error: No terminal session active.".to_string()
            }
        },
        "list_files" => {
            let dir = tool.arguments.get("dir_path").cloned();
            let effective_dir = if let Some(d) = dir {
                if d.is_empty() { root.clone() } else { root.join(d) }
            } else {
                root.clone()
            };

            match workspace_manager::build_file_tree(&root, &effective_dir) {
                Ok(entries) => {
                    entries.iter().map(|e| format!("{}{}", if e.is_dir { "[DIR] " } else { "" }, e.name)).collect::<Vec<_>>().join("\n")
                },
                Err(e) => format!("Error: {}", e)
            }
        },
        "read_file" => {
             let path = tool.arguments.get("file_path").cloned().unwrap_or_default();
             match workspace_manager::read_file_internal(&root, path) {
                 Ok(fc) => fc.content,
                 Err(e) => format!("Error: {}", e)
             }
        },
        "write_file" => {
             let path = tool.arguments.get("file_path").cloned().unwrap_or_default();
             let content = tool.arguments.get("content").cloned().unwrap_or_default();
             match workspace_manager::write_file_internal(&root, path, content) {
                 Ok(_) => "Successfully wrote file.".to_string(),
                 Err(e) => format!("Error: {}", e)
             }
        },
        "search_code" => {
             let query = tool.arguments.get("query").cloned().unwrap_or_default();
             match workspace_manager::search_code_internal(&root, &query) {
                 Ok(matches) => {
                     if matches.len() > 20 {
                         format!("Found {} matches. First 20:\n{}", matches.len(), matches[..20].join("\n"))
                     } else {
                         matches.join("\n")
                     }
                 },
                 Err(e) => format!("Error: {}", e)
             }
        },
        _ => format!("Unknown Tool: {}", tool.name)
    }
}
