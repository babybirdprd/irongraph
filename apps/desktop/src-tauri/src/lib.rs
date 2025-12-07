use tauri_specta::{collect_commands, Builder};
use shared_db::DbPool;
use specta_typescript::Typescript;
use std::sync::{Arc, Mutex};
use tauri::{State, Window, Emitter, Manager};
use agent_core::{AgentSession, spawn_agent_loop};
use common::WorkspaceState;
use terminal_manager::{TerminalState, ShellError};
use llm_gateway::LLMConfig;

#[tauri::command]
#[specta::specta]
async fn write_terminal(
    state: State<'_, Arc<TerminalState>>,
    session_id: String,
    input: String
) -> Result<(), ShellError> {
    terminal_manager::write_to_pty(state.inner(), &session_id, &input)
}

// Wrapper command to start agent
#[tauri::command]
#[specta::specta]
async fn start_agent_loop(
    window: Window,
    session_state: State<'_, Arc<AgentSession>>,
    workspace_state: State<'_, WorkspaceState>,
    terminal_state: State<'_, Arc<TerminalState>>,
    prompt: String
) -> Result<String, String> {
    let session = session_state.inner().clone();

    let is_running = session.status.load(std::sync::atomic::Ordering::Relaxed);

    if is_running {
        {
            let mut history = session.history.lock().unwrap();
            history.push(llm_gateway::Message { role: "user".into(), content: prompt.clone() });
        }
    }

    if !is_running {
         let config = LLMConfig {
             api_key: "sk-dummy".into(),
             base_url: "mock".into(),
             model: "gpt-4o".into(),
             temperature: 0.0,
         };

         let ws_arc = workspace_state.0.clone();
         let term_arc = terminal_state.inner().clone();

        spawn_agent_loop(
            window.clone(),
            session.clone(),
            ws_arc,
            term_arc,
            prompt,
            config
        ).await;
    }

    Ok(session.id.clone())
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            feature_profile::commands::update_profile,
            llm_gateway::commands::send_chat,
            workspace_manager::commands::list_files,
            workspace_manager::commands::read_file,
            workspace_manager::commands::write_file,
            workspace_manager::commands::search_code,
            terminal_manager::commands::run_command,
            start_agent_loop,
            write_terminal // Added
        ]);

    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/bindings.ts")
        .expect("Failed to export typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(DbPool::new())
        .manage(common::WorkspaceState(Arc::new(std::sync::Mutex::new(std::env::current_dir().expect("Failed to get current directory")))))
        .manage(Arc::new(AgentSession::new()))
        .manage(Arc::new(TerminalState::default()))
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        let builder = Builder::<tauri::Wry>::new()
            .commands(collect_commands![
                feature_profile::commands::update_profile,
                llm_gateway::commands::send_chat,
                workspace_manager::commands::list_files,
                workspace_manager::commands::read_file,
                workspace_manager::commands::write_file,
                workspace_manager::commands::search_code,
                terminal_manager::commands::run_command,
                start_agent_loop,
                write_terminal
            ]);

        builder
            .export(Typescript::default(), "../src/bindings.ts")
            .expect("Failed to export typescript bindings");
    }
}
