use portable_pty::{CommandBuilder, MasterPty, PtyPair, PtySize, NativePtySystem, PtySystem, Child};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use common::WorkspaceState;
use tokio::sync::mpsc::Sender;

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Error, Debug, Serialize, Type)]
pub enum ShellError {
    #[error("IO Error: {0}")]
    Io(String),
    #[error("Command not found: {0}")]
    NotFound(String),
    #[error("PTY Error: {0}")]
    Pty(String),
}

pub struct PtySession {
    pub writer: Box<dyn Write + Send>,
    // We don't keep master here if we spawn a reader thread.
    // Keep child alive
    pub child: Box<dyn Child + Send + Sync>,
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Attempt to kill the process on drop
        let _ = self.child.kill();
    }
}

pub struct TerminalState {
    pub sessions: Mutex<HashMap<String, Arc<Mutex<PtySession>>>>,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

// Spawns a persistent shell (bash/cmd) and pipes output to `output_tx`.
pub fn start_terminal_session(
    root: &PathBuf,
    state: &Arc<TerminalState>,
    output_tx: Sender<String>,
) -> Result<String, ShellError> {
    let pty_system = NativePtySystem::default();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }).map_err(|e| ShellError::Pty(e.to_string()))?;

    #[cfg(target_os = "windows")]
    let cmd = CommandBuilder::new("cmd.exe");
    #[cfg(not(target_os = "windows"))]
    let mut cmd = CommandBuilder::new("/bin/bash");

    cmd.cwd(root);

    let child = pair.slave.spawn_command(cmd)
        .map_err(|e| ShellError::Pty(e.to_string()))?;

    drop(pair.slave);

    let id = uuid::Uuid::new_v4().to_string();

    let mut reader = pair.master.try_clone_reader().map_err(|e| ShellError::Pty(e.to_string()))?;
    let writer = pair.master.take_writer().map_err(|e| ShellError::Pty(e.to_string()))?;

    // Spawn Reader Thread
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let s = String::from_utf8_lossy(&buffer[..n]).to_string();
                    if output_tx.blocking_send(s).is_err() {
                        break; // Receiver dropped
                    }
                },
                Err(_) => break,
            }
        }
    });

    let session = PtySession {
        writer,
        child,
    };

    state.sessions.lock().unwrap().insert(id.clone(), Arc::new(Mutex::new(session)));

    Ok(id)
}

pub fn run_command_internal(
    root: &PathBuf,
    program: &str,
    args: &[String],
    terminal_state: &Arc<TerminalState>,
    session_id: &str, // Reuse this session if provided, or expect one exists
) -> Result<String, ShellError> {
    // We assume a persistent shell exists for the session_id.
    // If not, we error (caller must create one).

    let sessions = terminal_state.sessions.lock().unwrap();
    if let Some(session_arc) = sessions.get(session_id) {
        let mut session = session_arc.lock().unwrap();

        // Formulate command line. Simple join for now.
        // shlex::join would be better but simple space join works for proof of concept if we don't have shlex here.
        // We can just send program + args.
        let cmd_line = format!("{} {}\n", program, args.join(" "));

        session.writer.write_all(cmd_line.as_bytes()).map_err(|e| ShellError::Io(e.to_string()))?;
        session.writer.flush().map_err(|e| ShellError::Io(e.to_string()))?;

        Ok(format!("Command sent to session {}", session_id))
    } else {
        Err(ShellError::NotFound("Session not found. Call start_terminal_session first.".into()))
    }
}

pub fn write_to_pty(state: &Arc<TerminalState>, session_id: &str, input: &str) -> Result<(), ShellError> {
    let sessions = state.sessions.lock().unwrap();
    if let Some(session_arc) = sessions.get(session_id) {
        let mut session = session_arc.lock().unwrap();
        session.writer.write_all(input.as_bytes()).map_err(|e| ShellError::Io(e.to_string()))?;
        session.writer.flush().map_err(|e| ShellError::Io(e.to_string()))?;
        Ok(())
    } else {
        Err(ShellError::NotFound("Session ID".into()))
    }
}

pub fn kill_session(state: &Arc<TerminalState>, session_id: &str) -> Result<(), ShellError> {
    let mut sessions = state.sessions.lock().unwrap();
    if sessions.remove(session_id).is_some() {
        Ok(())
    } else {
        Err(ShellError::NotFound("Session ID".into()))
    }
}

pub mod commands {
    use super::*;
    use tauri::State;

    // This one-shot command is problematic for persistent PTY.
    // We'll reimplement it to spawn a temporary PTY, run, and wait.
    // BUT user wants persistent.
    // If frontend calls `run_command`, maybe it expects blocking output?
    // Existing frontend tools use `run_command` and expect output.
    // So we keep the OLD behavior (blocking, new session) for THIS command,
    // OR we upgrade it?
    // The instructions say "Replaced simple command execution with persistent PTY".
    // "Agent runs python3 input.py ... User sends input".
    // This implies `run_command` is the tool used by the agent.
    // So the Agent's `run_command` MUST use the persistent session.
    // The Tauri command `run_command` might be legacy?
    // But `agent_core` calls `run_command_internal`.

    // We will leave this Tauri command as a legacy wrapper (non-persistent) or update it.
    // For safety, let's make it spawn a one-off PTY and return output, similar to before but via PTY.
    #[tauri::command]
    #[specta::specta]
    pub async fn run_command(state: State<'_, WorkspaceState>, program: String, args: Vec<String>) -> Result<CommandOutput, ShellError> {
        let root = state.0.lock().map_err(|_| ShellError::Io("Lock poison".into()))?.clone();

        // One-off PTY
        let pty_system = NativePtySystem::default();
        let pair = pty_system.openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).map_err(|e| ShellError::Pty(e.to_string()))?;
        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&args);
        cmd.cwd(root);
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| ShellError::Pty(e.to_string()))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| ShellError::Pty(e.to_string()))?;
        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap_or(0); // ignore err
        let exit = child.wait().map_err(|e| ShellError::Pty(e.to_string()))?;

        Ok(CommandOutput {
            stdout: output,
            stderr: "".into(),
            exit_code: if exit.success() { 0 } else { 1 }
        })
    }
}
