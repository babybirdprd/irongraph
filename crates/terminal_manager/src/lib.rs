use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;
use common::WorkspaceState;

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
}

pub mod commands {
    use super::*;
    use tauri::State;
    use std::process::Stdio;

    #[tauri::command]
    #[specta::specta]
    pub async fn run_command(state: State<'_, WorkspaceState>, program: String, args: Vec<String>) -> Result<CommandOutput, ShellError> {
        let root = state.0.lock().map_err(|_| ShellError::Io("Lock poison".into()))?.clone();

        let output = tokio::process::Command::new(&program)
            .args(&args)
            .current_dir(&root)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ShellError::NotFound(program.clone())
                } else {
                    ShellError::Io(e.to_string())
                }
            })?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}
