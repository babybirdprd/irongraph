use radkit::macros::tool;
use radkit::tools::{ToolResult, ToolContext};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use crate::{read_file_internal, write_file_internal, build_file_tree, search_code_internal, get_skeleton};
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
        if let Ok(matches) = crate::search_code_internal(root, &query) {
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

fn get_state(ctx: &ToolContext) -> Result<std::sync::Arc<RadkitState>, String> {
    let session_id_val = ctx.state().get_state("session_id").ok_or("No session_id in context")?;
    let session_id = session_id_val.as_str().ok_or("Invalid session_id type")?;
    get_session(session_id).ok_or("Session expired or not found".to_string())
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadFileArgs {
    pub file_path: String,
}

#[tool(description = "Read file content.")]
pub async fn read_file(args: ReadFileArgs, ctx: &ToolContext<'_>) -> ToolResult {
    let state = match get_state(ctx) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    match read_file_internal(&state.root, args.file_path) {
        Ok(fc) => ToolResult::success(fc.content.into()),
        Err(e) => ToolResult::error(format!("Error: {}", e))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteFileArgs {
    pub file_path: String,
    pub content: String,
}

#[tool(description = "Write file content. Will analyze imports to warn about potential breakages.")]
pub async fn write_file(args: WriteFileArgs, ctx: &ToolContext<'_>) -> ToolResult {
    let state = match get_state(ctx) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    match write_file_internal(&state.root, args.file_path.clone(), args.content) {
        Ok(_) => {
            let mut output = "Successfully wrote file.".to_string();
            if let Some(consumers) = find_usages(&state.root, &args.file_path) {
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
            ToolResult::success(output.into())
        },
        Err(e) => ToolResult::error(format!("Error: {}", e))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ListFilesArgs {
    pub dir_path: Option<String>,
}

#[tool(description = "List files in the directory.")]
pub async fn list_files(args: ListFilesArgs, ctx: &ToolContext<'_>) -> ToolResult {
    let state = match get_state(ctx) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    let effective_dir = if let Some(d) = args.dir_path {
        if d.is_empty() { state.root.clone() } else { state.root.join(d) }
    } else {
        state.root.clone()
    };

    match build_file_tree(&state.root, &effective_dir) {
        Ok(entries) => {
             let s = entries.iter().map(|e| format!("{}{}", if e.is_dir { "[DIR] " } else { "" }, e.name)).collect::<Vec<_>>().join("\n");
             ToolResult::success(s.into())
        },
        Err(e) => ToolResult::error(format!("Error: {}", e))
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadSkeletonArgs {
    pub file_path: String,
}

#[tool(description = "Read the skeleton of a file (structure without function bodies).")]
pub async fn read_skeleton(args: ReadSkeletonArgs, ctx: &ToolContext<'_>) -> ToolResult {
    let state = match get_state(ctx) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    let fc = read_file_internal(&state.root, args.file_path.clone());
    match fc {
        Ok(c) => match get_skeleton(std::path::Path::new(&args.file_path), &c.content) {
            Ok(s) => ToolResult::success(s.into()),
            Err(e) => ToolResult::error(format!("Error generating skeleton: {}", e)),
        },
        Err(e) => ToolResult::error(format!("Error reading file: {}", e)),
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchCodeArgs {
    pub query: String,
}

#[tool(description = "Search code using regex.")]
pub async fn search_code(args: SearchCodeArgs, ctx: &ToolContext<'_>) -> ToolResult {
    let state = match get_state(ctx) {
        Ok(s) => s,
        Err(e) => return ToolResult::error(e),
    };

    match search_code_internal(&state.root, &args.query) {
        Ok(matches) => {
            if matches.len() > 20 {
                let s = format!("Found {} matches. First 20:\n{}", matches.len(), matches[..20].join("\n"));
                ToolResult::success(s.into())
            } else {
                ToolResult::success(matches.join("\n").into())
            }
        },
        Err(e) => ToolResult::error(format!("Error: {}", e))
    }
}
