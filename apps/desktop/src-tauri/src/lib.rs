use tauri_specta::{collect_commands, Builder};
use specta_typescript::Typescript;
use std::sync::{Arc, Mutex};
use tauri::{State, Window, Emitter, Manager};
use agent_core::{AgentSession, spawn_agent_loop};
use common::WorkspaceState;
use terminal_manager::{TerminalState, ShellError};
use llm_gateway::LLMConfig;

mod db;
use db::SqliteHistory;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::PathBuf;

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
        // Note: We also need to save to DB here if we want persistence of User messages
        // sent while running.
        // The `spawn_agent_loop` only saves messages it processes.
        // If we just push to in-memory history, the loop will read it.
        // But for consistency, we should add to repository.
        let msg = llm_gateway::Message { role: "user".into(), content: prompt.clone() };
        let _ = session.repository.add_message(&session.id, msg.clone()); // Async call ignored?
        // Wait, `add_message` is async. We are in async fn. We should await.
        let _ = session.repository.add_message(&session.id, msg.clone()).await;

        {
            let mut history = session.history.lock().unwrap();
            history.push(msg);
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
        .plugin(tauri_plugin_sql::Builder::default().build())
        // Remove shared_db pool
        .manage(common::WorkspaceState(Arc::new(std::sync::Mutex::new(std::env::current_dir().expect("Failed to get current directory")))))
        .manage(Arc::new(TerminalState::default()))
        // Setup AgentSession with DB
        .setup(move |app| {
            builder.mount_events(app);

            let app_handle = app.handle().clone();

            tauri::async_runtime::block_on(async move {
                // Initialize DB connection for Backend Use
                // Note: tauri-plugin-sql handles the file creation via frontend or its own setup?
                // The instructions say "Frontend: The UI now uses the plugin...".
                // We should ensure the DB exists.
                // tauri-plugin-sql doesn't necessarily create it until accessed.
                // We will resolve the path manually.

                let app_dir = app_handle.path().app_data_dir().expect("failed to get app data dir");
                if !app_dir.exists() {
                    std::fs::create_dir_all(&app_dir).expect("failed to create app data dir");
                }
                let db_path = app_dir.join("irongraph.db");
                let db_url = format!("sqlite://{}", db_path.to_string_lossy());

                // Create pool
                if !db_path.exists() {
                    use std::fs::File;
                    File::create(&db_path).expect("failed to create db file");
                }

                let pool = SqlitePoolOptions::new()
                    .connect(&db_url)
                    .await
                    .expect("Failed to connect to backend DB pool");

                // Run migrations? tauri-plugin-sql can run migrations if configured in Rust,
                // but usually it's configured in `tauri.conf.json` or via `Builder`.
                // The instructions said "Added migrations/20250101_init.sql".
                // tauri-plugin-sql builder can take migrations.
                // Let's assume the plugin handles it if we registered it correctly?
                // Wait, I didn't add migrations to `tauri_plugin_sql::Builder`.
                // I should do that in the main builder chain above, BUT I am inside setup.
                // Actually, if I use `tauri_plugin_sql::Builder::default().add_migrations("sqlite:irongraph.db", migrations)...`
                // But simpler: just run migration here with sqlx.

                sqlx::query(include_str!("../migrations/20250101_init.sql"))
                    .execute(&pool)
                    .await
                    .expect("Failed to run migrations");

                let history = SqliteHistory::new(pool);
                let terminal_state = app_handle.state::<Arc<TerminalState>>(); // This works if managed before setup?
                // Yes, manage happens before setup.

                // But `app_handle.state` returns a wrapper.
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
                terminal_manager::commands::run_command,
                start_agent_loop,
                write_terminal
            ]);

        builder
            .export(Typescript::default(), "../src/bindings.ts")
            .expect("Failed to export typescript bindings");
    }
}
