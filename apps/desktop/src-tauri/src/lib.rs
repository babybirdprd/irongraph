use tauri_specta::{collect_commands, Builder};
use specta_typescript::Typescript;
use std::sync::{Arc, Mutex};
use tauri::{State, Window, Emitter, Manager};
use agent_core::{AgentSession, spawn_agent_loop, LLMConfig};
use common::WorkspaceState;
use terminal_manager::{common::TerminalState, ShellError};

mod db;
use db::SqliteHistory;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::PathBuf;

const OPENROUTER_KEY: &str = "";

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
        // Just append to history, loop picks it up
        let msg = serde_json::json!({ "role": "user", "content": prompt });
        let _ = session.repository.add_message(&session.id, msg).await;
    }

    if !is_running {
         // TODO: Pass actual config from UI state or DB?
         // For now, loading from Env inside core is fine if config here is dummy,
         // BUT reviewer requested config restoration.
         // We will pass env vars if possible or default.
         let config = LLMConfig {
             api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or(OPENROUTER_KEY.to_string()),
             model: "deepseek/deepseek-v3.2".to_string(),
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
            workspace_manager::commands::read_skeleton, // Restored binding
            terminal_manager::commands::run_command,
            start_agent_loop,
            write_terminal
        ]);

    #[cfg(debug_assertions)]
    builder
        .export(Typescript::default(), "../src/bindings.ts")
        .expect("Failed to export typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .manage(common::WorkspaceState(Arc::new(std::sync::Mutex::new(std::env::current_dir().expect("Failed to get current directory")))))
        .manage(Arc::new(TerminalState::default()))
        .setup(move |app| {
            builder.mount_events(app);

            let app_handle = app.handle().clone();

            tauri::async_runtime::block_on(async move {
                let app_dir = app_handle.path().app_data_dir().expect("failed to get app data dir");
                if !app_dir.exists() {
                    std::fs::create_dir_all(&app_dir).expect("failed to create app data dir");
                }
                let db_path = app_dir.join("irongraph.db");
                let db_url = format!("sqlite://{}", db_path.to_string_lossy());

                if !db_path.exists() {
                    use std::fs::File;
                    File::create(&db_path).expect("failed to create db file");
                }

                let pool = SqlitePoolOptions::new()
                    .connect(&db_url)
                    .await
                    .expect("Failed to connect to backend DB pool");

                sqlx::query(include_str!("../migrations/20250101_init.sql"))
                    .execute(&pool)
                    .await
                    .expect("Failed to run migrations");

                let history = SqliteHistory::new(pool);
                let terminal_state = app_handle.state::<Arc<TerminalState>>();
                let ts = terminal_state.inner().clone();

                let session = AgentSession::new(Box::new(history), ts);
                app_handle.manage(Arc::new(session));
            });

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
                workspace_manager::commands::read_skeleton,
                terminal_manager::commands::run_command,
                start_agent_loop,
                write_terminal
            ]);

        builder
            .export(Typescript::default(), "../src/bindings.ts")
            .expect("Failed to export typescript bindings");
    }
}
