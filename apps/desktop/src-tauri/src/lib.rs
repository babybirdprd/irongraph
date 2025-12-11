use tauri_specta::{collect_commands, Builder};
use specta_typescript::Typescript;
use std::sync::{Arc, Mutex};
use tauri::{State, Window, Emitter, Manager};
use agent_core::{AgentSession, spawn_agent_loop, LLMConfig as AgentLLMConfig};
use common::WorkspaceState;
use terminal_manager::{common::TerminalState};

mod db;
use db::SqliteHistory;
use sqlx::sqlite::SqlitePoolOptions;
use std::path::{Path, PathBuf};

// Protocol Imports
use irongraph_protocol::{
    FileEntry as ApiFileEntry,
    FileContent as ApiFileContent,
    FsError as ApiFsError,
    CommandOutput as ApiCommandOutput,
    ShellError as ApiShellError,
    UpdateProfileReq as ApiUpdateProfileReq,
    UserProfile as ApiUserProfile,
    LLMRequest as ApiLLMRequest,
    LLMResponse as ApiLLMResponse,
    LLMConfig as ApiLLMConfig,
    Message as ApiMessage,
    ToolCall as ApiToolCall
};

// Logic Imports
use workspace_manager::{
    FileEntry as LogicFileEntry,
    FileContent as LogicFileContent,
    FsError as LogicFsError
};
use terminal_manager::{
    CommandOutput as LogicCommandOutput,
    ShellError as LogicShellError
};
use llm_gateway::{
    LLMRequest as LogicLLMRequest,
    LLMResponse as LogicLLMResponse,
    LLMConfig as LogicLLMConfig,
    Message as LogicMessage,
    ToolCall as LogicToolCall
};
use shared_db::UserProfile as LogicUserProfile;

const OPENROUTER_KEY: &str = "";

// ============================================================================
// Mappers
// ============================================================================

fn map_fs_error(e: LogicFsError) -> ApiFsError {
    match e {
        LogicFsError::Io(err) => ApiFsError::Io(err.to_string()),
        LogicFsError::SecurityViolation => ApiFsError::SecurityViolation,
        LogicFsError::InvalidPath => ApiFsError::InvalidPath,
        LogicFsError::Syntax(msg) => ApiFsError::Syntax(msg),
    }
}

fn map_file_entry(e: LogicFileEntry) -> ApiFileEntry {
    ApiFileEntry {
        path: e.path.to_string_lossy().to_string(),
        name: e.name,
        is_dir: e.is_dir,
        children: e.children.map(|c| c.into_iter().map(map_file_entry).collect()),
    }
}

fn map_file_content(c: LogicFileContent) -> ApiFileContent {
    ApiFileContent {
        path: c.path.to_string_lossy().to_string(),
        content: c.content
    }
}

fn map_shell_error(e: LogicShellError) -> ApiShellError {
    match e {
        LogicShellError::Io(msg) => ApiShellError::Io(msg),
        LogicShellError::NotFound(msg) => ApiShellError::NotFound(msg),
        LogicShellError::Pty(msg) => ApiShellError::Pty(msg),
    }
}

fn map_command_output(o: LogicCommandOutput) -> ApiCommandOutput {
    ApiCommandOutput {
        stdout: o.stdout,
        stderr: o.stderr,
        exit_code: o.exit_code,
    }
}

fn map_user_profile(p: LogicUserProfile) -> ApiUserProfile {
    ApiUserProfile {
        id: p.id,
        name: p.name,
        bio: p.bio,
    }
}

// LLM Mappers - Deep Mapping required
fn map_llm_req_to_logic(req: ApiLLMRequest) -> LogicLLMRequest {
    LogicLLMRequest {
        messages: req.messages.into_iter().map(|m| LogicMessage {
            role: m.role,
            content: m.content,
        }).collect(),
        config: LogicLLMConfig {
            api_key: req.config.api_key,
            base_url: req.config.base_url,
            model: req.config.model,
            temperature: req.config.temperature,
        }
    }
}

fn map_llm_res_to_api(res: LogicLLMResponse) -> ApiLLMResponse {
    ApiLLMResponse {
        role: res.role,
        content: res.content,
        tool_calls: res.tool_calls.map(|t| t.into_iter().map(|tc| ApiToolCall {
            name: tc.name,
            arguments: tc.arguments,
        }).collect()),
        usage: res.usage,
    }
}


// ============================================================================
// Commands
// ============================================================================

#[tauri::command]
#[specta::specta]
async fn list_files(state: State<'_, WorkspaceState>, dir_path: Option<String>) -> Result<Vec<ApiFileEntry>, ApiFsError> {
    let root = state.0.lock().map_err(|_| ApiFsError::Io("Lock poison".into()))?.clone();
    let start_dir = if let Some(sub) = dir_path {
         // Re-implement path validation call or just pass string?
         // Logic `build_file_tree` takes Path.
         // We need to resolve start_dir relative to root securely.
         // Wait, `workspace_manager::validate_path` is private.
         // I should have exposed `validate_path` or made a helper in `workspace_manager`.
         // Current `workspace_manager` exposes `read_file_internal` which does validation.
         // `build_file_tree` takes `current_dir: &Path`.
         // Let's assume input `dir_path` is relative to root.
         // Ideally `workspace_manager` should handle the safe resolution.
         // I'll assume for now `build_file_tree` expects absolute path but checks safety?
         // No, `build_file_tree` in `workspace_manager` assumes `current_dir` is valid.

         // Fix: I need to use `workspace_manager` to resolve the path SAFELY.
         // But `validate_path` is private.
         // I will trust the logic in `workspace_manager::read_file_internal` style.
         // Actually, `workspace_manager::build_file_tree` iterates `current_dir`.
         // I need to resolve `root.join(dir_path)` securely.
         // I will modify `workspace_manager` to expose a safe `resolve_path` or `list_files_safe`.
         // OR I can duplicate the simple check here.

         // Actually, I should update `workspace_manager` to expose a function `list_files_safe(root, relative_path)`.
         // But for now, to avoid context switching back and forth too much:
         // I'll implement basic check here or use `std::fs::canonicalize`.
         let p = root.join(sub);
         if let Ok(canon) = p.canonicalize() {
             if !canon.starts_with(&root) {
                 return Err(ApiFsError::SecurityViolation);
             }
             canon
         } else {
             // Does not exist?
             return Err(ApiFsError::InvalidPath);
         }
    } else {
         root.clone()
    };

    workspace_manager::build_file_tree(&root, &start_dir)
        .map_err(map_fs_error)
        .map(|entries| entries.into_iter().map(map_file_entry).collect())
}

#[tauri::command]
#[specta::specta]
async fn read_file(state: State<'_, WorkspaceState>, file_path: String) -> Result<ApiFileContent, ApiFsError> {
    let root = state.0.lock().map_err(|_| ApiFsError::Io("Lock poison".into()))?.clone();
    workspace_manager::read_file_internal(&root, file_path)
        .map_err(map_fs_error)
        .map(map_file_content)
}

#[tauri::command]
#[specta::specta]
async fn write_file(state: State<'_, WorkspaceState>, file_path: String, content: String) -> Result<ApiFileContent, ApiFsError> {
     let root = state.0.lock().map_err(|_| ApiFsError::Io("Lock poison".into()))?.clone();
     workspace_manager::write_file_internal(&root, file_path, content)
        .map_err(map_fs_error)
        .map(map_file_content)
}

#[tauri::command]
#[specta::specta]
async fn search_code(state: State<'_, WorkspaceState>, query: String) -> Result<Vec<String>, ApiFsError> {
     let root = state.0.lock().map_err(|_| ApiFsError::Io("Lock poison".into()))?.clone();
     workspace_manager::search_code_internal(&root, &query)
        .map_err(map_fs_error)
}

#[tauri::command]
#[specta::specta]
async fn read_skeleton(state: State<'_, WorkspaceState>, file_path: String) -> Result<String, ApiFsError> {
    let root = state.0.lock().map_err(|_| ApiFsError::Io("Lock poison".into()))?.clone();
    workspace_manager::read_skeleton_internal(&root, file_path)
        .map_err(map_fs_error)
}

#[tauri::command]
#[specta::specta]
async fn run_command(state: State<'_, WorkspaceState>, program: String, args: Vec<String>) -> Result<ApiCommandOutput, ApiShellError> {
    let root = state.0.lock().map_err(|_| ApiShellError::Io("Lock poison".into()))?.clone();
    terminal_manager::run_command_internal(&root, program, args)
        .map_err(map_shell_error)
        .map(map_command_output)
}

#[tauri::command]
#[specta::specta]
async fn write_terminal(
    state: State<'_, Arc<TerminalState>>,
    session_id: String,
    input: String
) -> Result<(), ApiShellError> {
    terminal_manager::write_to_pty(state.inner(), &session_id, &input)
        .map_err(map_shell_error)
}

#[tauri::command]
#[specta::specta]
async fn update_profile(state: State<'_, shared_db::DbPool>, req: ApiUpdateProfileReq) -> Result<ApiUserProfile, String> {
    feature_profile::update_profile_logic(state.inner(), req.name, req.bio)
        .map(map_user_profile)
}

#[tauri::command]
#[specta::specta]
async fn send_chat(req: ApiLLMRequest) -> Result<ApiLLMResponse, String> {
    let logic_req = map_llm_req_to_logic(req);
    llm_gateway::send_chat_logic(logic_req).await
        .map(map_llm_res_to_api)
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
         let config = AgentLLMConfig {
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
            update_profile,
            send_chat,
            list_files,
            read_file,
            write_file,
            search_code,
            read_skeleton,
            run_command,
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

                let history = SqliteHistory::new(pool.clone());
                let terminal_state = app_handle.state::<Arc<TerminalState>>();
                let ts = terminal_state.inner().clone();

                // Also provide pool to state for feature_profile
                app_handle.manage(pool);

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
                update_profile,
                send_chat,
                list_files,
                read_file,
                write_file,
                search_code,
                read_skeleton,
                run_command,
                start_agent_loop,
                write_terminal
            ]);

        builder
            .export(Typescript::default(), "../src/bindings.ts")
            .expect("Failed to export typescript bindings");
    }
}
