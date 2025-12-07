use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use tauri::{Window, Emitter};
use llm_gateway::{LLMRequest, Message, LLMConfig, StreamEvent, ToolCall, stream_chat};
use futures::StreamExt;
use terminal_manager::TerminalState;
use tokio::sync::mpsc;
use async_trait::async_trait;

mod shell;
use shell::ShellType;

// Define HistoryRepository trait for persistence abstraction
#[async_trait]
pub trait HistoryRepository: Send + Sync {
    async fn add_message(&self, session_id: &str, message: Message) -> anyhow::Result<()>;
    async fn get_history(&self, session_id: &str) -> anyhow::Result<Vec<Message>>;
}

pub struct AgentSession {
    pub id: String,
    pub history: Arc<Mutex<Vec<Message>>>,
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
            history: Arc::new(Mutex::new(Vec::new())),
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
        if let Some(state) = &self.terminal_state {
            if let Ok(guard) = self.terminal_session_id.lock() {
                if let Some(id) = guard.as_ref() {
                    let _ = terminal_manager::kill_session(state, id);
                }
            }
        }
    }
}

const SYSTEM_PROMPT: &str = "You are IronGraph, an advanced AI software engineer...";

fn try_parse_error_context(root: &std::path::Path, stderr: &str) -> Option<String> {
    // Regex for Rust errors: `error[E...]: ... --> src/main.rs:10:5`
    // Regex for generic file:line:col: `path/to/file:line:col`
    // We try to catch common formats.

    // Rust: `--> file:line:col`
    let rust_re = regex::Regex::new(r"-->\s+(.+):(\d+):(\d+)").ok()?;

    // TS/JS/Generic: `file(line,col):` or `file:line:col:`
    // Note: This matches start of line or space
    let generic_re = regex::Regex::new(r"(?m)(?:^|\s)([\w./-]+):(\d+):(\d+)").ok()?;

    // TypeScript (tsc): `file.ts(12,3): error TS...`
    let ts_re = regex::Regex::new(r"([\w./-]+)\((\d+),\d+\):\s+error").ok()?;

    let mut location = None;

    if let Some(caps) = rust_re.captures(stderr) {
        if let (Some(f), Some(l)) = (caps.get(1), caps.get(2)) {
             location = Some((f.as_str().to_string(), l.as_str().parse::<usize>().unwrap_or(0)));
        }
    } else if let Some(caps) = ts_re.captures(stderr) {
        if let (Some(f), Some(l)) = (caps.get(1), caps.get(2)) {
             location = Some((f.as_str().to_string(), l.as_str().parse::<usize>().unwrap_or(0)));
        }
    } else if let Some(caps) = generic_re.captures(stderr) {
         if let (Some(f), Some(l)) = (caps.get(1), caps.get(2)) {
             // Basic filter to avoid matching random text like "http://localhost:3000" (which has :)
             // Although regex requires digit after :
             let path = f.as_str();
             if path.contains('.') { // Assume file has extension
                 location = Some((path.to_string(), l.as_str().parse::<usize>().unwrap_or(0)));
             }
        }
    }

    if let Some((file, line)) = location {
        // Read window around line
        if let Ok(fc) = workspace_manager::read_file_internal(root, file.clone()) {
            let lines: Vec<&str> = fc.content.lines().collect();
            if line > 0 && line <= lines.len() {
                let start = if line > 5 { line - 5 } else { 0 };
                let end = if line + 5 < lines.len() { line + 5 } else { lines.len() };

                let snippet = lines[start..end].iter().enumerate().map(|(i, l)| {
                    let curr_line = start + i + 1;
                    let marker = if curr_line == line { ">> " } else { "   " };
                    format!("{}{}| {}", marker, curr_line, l)
                }).collect::<Vec<_>>().join("\n");

                return Some(format!("File: {}:{}:\n{}", file, line, snippet));
            }
        }
    }

    None
}

fn find_usages(root: &std::path::Path, file_path: &str) -> Option<Vec<String>> {
    let path_obj = std::path::Path::new(file_path);
    let extension = path_obj.extension().and_then(|e| e.to_str()).unwrap_or("");

    let search_term = if extension == "rs" {
        // Rust strategy
        if let Some(stem) = path_obj.file_stem().and_then(|s| s.to_str()) {
            if stem == "mod" {
                // src/foo/mod.rs -> foo
                path_obj.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).map(|s| s.to_string())
            } else {
                Some(stem.to_string())
            }
        } else {
            None
        }
    } else if ["ts", "tsx", "js", "jsx"].contains(&extension) {
        // JS/TS Strategy: naive import check (filename without extension)
        path_obj.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
    } else {
        None
    };

    if let Some(term) = search_term {
        let query = format!(r"\b{}\b", regex::escape(&term));
        if let Ok(matches) = workspace_manager::search_code_internal(root, &query) {
             let mut consumers = Vec::new();
             for m in matches {
                 // m format: path:line: content
                 if let Some((path_part, _)) = m.split_once(':') {
                     if path_part != file_path && !consumers.contains(&path_part.to_string()) {
                         consumers.push(path_part.to_string());
                     }
                 }
             }
             return Some(consumers);
        }
    }
    None
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

    // Load history from DB if empty
    {
        // Check if history is empty (locked)
        let is_empty = session.history.lock().unwrap().is_empty();

        if is_empty {
             match session.repository.get_history(&session_id).await {
                 Ok(msgs) if !msgs.is_empty() => {
                     let mut history = session.history.lock().unwrap();
                     *history = msgs;
                 },
                 _ => {
                     // New Session
                     let sys = Message { role: "system".into(), content: SYSTEM_PROMPT.into() };
                     let _ = session.repository.add_message(&session_id, sys.clone()).await;
                     let mut history = session.history.lock().unwrap();
                     history.push(sys);
                 }
             }
        }

        // Add User Prompt
        let user_msg = Message { role: "user".into(), content: initial_prompt.clone() };
        let _ = session.repository.add_message(&session_id, user_msg.clone()).await;
        let mut history = session.history.lock().unwrap();
        history.push(user_msg);
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

            // Context Inspection
            if let Ok(bpe) = tiktoken_rs::cl100k_base() {
                 let mut full_text = String::new();
                 for m in &req.messages {
                     full_text.push_str(&m.role);
                     full_text.push_str(&m.content);
                 }
                 let tokens = bpe.encode_with_special_tokens(&full_text).len();
                 let _ = window.emit(&format!("agent:debug:stats:{}", session_id), tokens);
            }

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

            let asst_msg = Message {
                role: "assistant".into(),
                content: assistant_content.clone(),
            };
            let _ = session.repository.add_message(&session_id, asst_msg.clone()).await;

            {
                let mut history = session.history.lock().unwrap();
                history.push(asst_msg);
            }

            if tool_calls.is_empty() {
                let _ = window.emit(&format!("agent:status:{}", session_id), "waiting");
                session.status.store(false, Ordering::Relaxed);
                break;
            }

            for tool in tool_calls {
                let output = execute_tool(&tool, &session, &workspace_state, &terminal_state).await;

                let result_msg = format!("Tool Output [{}]:\n{}", tool.name, output);

                let tool_msg = Message {
                    role: "user".into(),
                    content: result_msg.clone(),
                };
                let _ = session.repository.add_message(&session_id, tool_msg.clone()).await;

                {
                    let mut history = session.history.lock().unwrap();
                    history.push(tool_msg);
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
                 let (tx, mut rx) = mpsc::channel(100);
                 {
                     let mut buf_lock = session.command_buffer.lock().unwrap();
                     *buf_lock = Some(tx);
                 }

                 let cmd_str = if args.is_empty() {
                     program
                 } else {
                     format!("{} {}", program, args.join(" "))
                 };

                 #[cfg(target_os = "windows")]
                 let shell_type = ShellType::Cmd;
                 #[cfg(not(target_os = "windows"))]
                 let shell_type = ShellType::Bash;

                 let sentinel_cmd = shell_type.format_with_sentinel(&cmd_str);

                 if let Err(e) = terminal_manager::write_to_pty(terminal_state_arc, &tid, &sentinel_cmd) {
                     return format!("Error writing to PTY: {}", e);
                 }

                 let mut output = String::new();
                 let start = std::time::Instant::now();
                 let timeout = std::time::Duration::from_secs(30);

                 loop {
                     let chunk = match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
                         Ok(Some(s)) => s,
                         Ok(None) => break,
                         Err(_) => {
                             if start.elapsed() > timeout {
                                 output.push_str("\n[IronGraph: Timeout waiting for sentinel]");
                                 break;
                             }
                             continue;
                         }
                     };

                     output.push_str(&chunk);

                     if let Some(idx) = output.find("IRONGRAPH_CMD_DONE:") {
                         let ret = output[..idx].to_string();
                         let rest = &output[idx..];
                         let code_str = rest.trim_start_matches("IRONGRAPH_CMD_DONE:").trim();
                         let exit_code = code_str.parse::<i32>().unwrap_or(1);

                         {
                             let mut buf_lock = session.command_buffer.lock().unwrap();
                             *buf_lock = None;
                         }

                         let mut final_output = format!("{}\n(Exit Code: {})", ret.trim(), exit_code);

                         if exit_code != 0 {
                             if let Some(debug_ctx) = try_parse_error_context(&root, &ret) {
                                 final_output.push_str(&format!("\n\n[Auto-Debug] Context:\n{}", debug_ctx));
                             }
                         }

                         return final_output;
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
             match workspace_manager::write_file_internal(&root, path.clone(), content) {
                 Ok(_) => {
                     // Dependency Injection Check
                     let mut output = "Successfully wrote file.".to_string();
                     if let Some(consumers) = find_usages(&root, &path) {
                         if !consumers.is_empty() {
                             output.push_str("\n\n[Context Note] This file is imported by:\n");
                             for c in consumers.iter().take(10) {
                                 output.push_str(&format!("- {}\n", c));
                             }
                             if consumers.len() > 10 {
                                 output.push_str(&format!("... and {} more.\n", consumers.len() - 10));
                             }
                             output.push_str("Ensure you have not broken these consumers.");
                         }
                     }
                     output
                 },
                 Err(e) => format!("Error: {}", e)
             }
        },
        "read_skeleton" => {
            let path = tool.arguments.get("file_path").cloned().unwrap_or_default();
            let fc = workspace_manager::read_file_internal(&root, path.clone());
            match fc {
                Ok(c) => match workspace_manager::get_skeleton(std::path::Path::new(&path), &c.content) {
                    Ok(s) => s,
                    Err(e) => format!("Error generating skeleton: {}", e),
                },
                Err(e) => format!("Error reading file: {}", e),
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
