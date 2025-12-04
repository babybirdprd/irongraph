use tauri_specta::{collect_commands, Builder};
use shared_db::DbPool;
use specta_typescript::Typescript;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            feature_profile::commands::update_profile,
            llm_gateway::commands::send_chat
        ]);

    #[cfg(debug_assertions)] // Only export TS bindings in dev mode
    builder
        .export(Typescript::default(), "../src/bindings.ts")
        .expect("Failed to export typescript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(DbPool::new()) // Manage the state
        .invoke_handler(builder.invoke_handler()) // Connect Specta to Tauri
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
                llm_gateway::commands::send_chat
            ]);

        // Path relative to Cargo.toml of the crate running the test
        builder
            .export(Typescript::default(), "../src/bindings.ts")
            .expect("Failed to export typescript bindings");
    }
}
