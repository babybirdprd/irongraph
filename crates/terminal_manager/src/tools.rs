use radkit::macros::tool;
use radkit::tools::{ToolResult, ToolContext};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use crate::{write_to_pty};
use common::{get_session, RadkitState};

// Hack for missing to_value
trait ToValueExt {
    fn to_value(&self) -> serde_json::Value;
}
impl ToValueExt for schemars::schema::RootSchema {
    fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
}

pub enum ShellType {
    Bash,
    Cmd,
    PowerShell,
}

impl ShellType {
    pub fn format_with_sentinel(&self, command: &str) -> String {
        match self {
            // Unix: Use semicolon and $?
            Self::Bash => format!("{}; echo \"IRONGRAPH_CMD_DONE:$?\"\n", command),
            // Windows CMD: Use ampersand and %ERRORLEVEL%
            Self::Cmd => format!("{} & echo IRONGRAPH_CMD_DONE:%ERRORLEVEL%\r\n", command),
            // PowerShell: Use semicolon and $LASTEXITCODE
            Self::PowerShell => format!("{}; Write-Host \"IRONGRAPH_CMD_DONE:$LASTEXITCODE\"\r\n", command),
        }
    }
}

fn try_parse_error_context(root: &std::path::Path, stderr: &str) -> Option<String> {
    // Rust: `--> file:line:col`
    let rust_re = regex::Regex::new(r"-->\s+(.+):(\d+):(\d+)").ok()?;
    // TS/Generic: `file(line,col):` or `file:line:col:`
    let generic_re = regex::Regex::new(r"(?m)(?:^|\s)([\w./-]+):(\d+):(\d+)").ok()?;
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
             let path = f.as_str();
             if path.contains('.') {
                 location = Some((path.to_string(), l.as_str().parse::<usize>().unwrap_or(0)));
             }
        }
    }

    if let Some((file, line)) = location {
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

fn get_state(ctx: &ToolContext) -> Result<std::sync::Arc<RadkitState>, String> {
    let session_id_val = ctx.state().get_state("session_id").ok_or("No session_id in context")?;
    let session_id = session_id_val.as_str().ok_or("Invalid session_id type")?;
    get_session(session_id).ok_or("Session expired or not found".to_string())
}

#[derive(Deserialize, JsonSchema)]
pub struct RunCommandArgs {
    pub program: String,
    #[serde(default)]
    pub args: Option<String>,
}

#[tool(description = "Run a shell command. Use this for all execution.")]
pub async fn run_command(args: RunCommandArgs, ctx: &ToolContext<'_>) -> ToolResult {
    let state = match get_state(ctx) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    let args_vec = shlex::split(&args.args.unwrap_or_default()).unwrap_or_default();

    let cmd_str = if args_vec.is_empty() {
        args.program.clone()
    } else {
        format!("{} {}", args.program, args_vec.join(" "))
    };

    #[cfg(target_os = "windows")]
    let shell_type = ShellType::Cmd;
    #[cfg(not(target_os = "windows"))]
    let shell_type = ShellType::Bash;

    let sentinel_cmd = shell_type.format_with_sentinel(&cmd_str);

    // Setup interception
    let (tx, mut rx) = mpsc::channel(100);
    {
        let mut buf_lock = state.command_buffer.lock().unwrap();
        *buf_lock = Some(tx);
    }

    if let Err(e) = write_to_pty(&state.terminal_state, &state.session_id, &sentinel_cmd) {
         return ToolResult::error(format!("Error writing to PTY: {}", e));
    }

    let mut output = String::new();
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(60); // 60s timeout

    loop {
         let chunk = match tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await {
             Ok(Some(s)) => s,
             Ok(None) => break, // Channel closed
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

             // Cleanup
             {
                 let mut buf_lock = state.command_buffer.lock().unwrap();
                 *buf_lock = None;
             }

             let mut final_output = format!("{}\n(Exit Code: {})", ret.trim(), exit_code);

             if exit_code != 0 {
                 if let Some(debug_ctx) = try_parse_error_context(&state.root, &ret) {
                     final_output.push_str(&format!("\n\n[Auto-Debug] Context:\n{}", debug_ctx));
                 }
             }

             return ToolResult::success(final_output.into());
         }
    }

    // Cleanup if timeout or break
    {
        let mut buf_lock = state.command_buffer.lock().unwrap();
        *buf_lock = None;
    }

    ToolResult::success(output.into())
}
