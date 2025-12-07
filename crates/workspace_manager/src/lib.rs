use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use thiserror::Error;
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, sinks::UTF8};
use ignore::WalkBuilder;
use syn::parse_file;

mod skeleton;
pub use skeleton::get_skeleton;

pub mod tools;

pub use common::WorkspaceState;

#[derive(Error, Debug, Serialize, Type)]
pub enum FsError {
    #[error("IO Error: {0}")]
    Io(String),
    #[error("Security Violation: Path traversal detected")]
    SecurityViolation,
    #[error("Invalid Path")]
    InvalidPath,
    #[error("Syntax Error: {0}")]
    Syntax(String),
}

impl From<std::io::Error> for FsError {
    fn from(e: std::io::Error) -> Self {
        FsError::Io(e.to_string())
    }
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileEntry>>,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct FileContent {
    pub path: String,
    pub content: String,
}

fn validate_path(base: &Path, user_path: &str, require_exists: bool) -> Result<PathBuf, FsError> {
    let path_parts = Path::new(user_path);
    for component in path_parts.components() {
        if let std::path::Component::ParentDir = component {
            return Err(FsError::SecurityViolation);
        }
    }

    let full_path = base.join(user_path);

    if require_exists {
        let canonical_path = full_path.canonicalize().map_err(|e| FsError::Io(e.to_string()))?;
        let canonical_base = base.canonicalize().map_err(|e| FsError::Io(e.to_string()))?;

        if !canonical_path.starts_with(&canonical_base) {
             return Err(FsError::SecurityViolation);
        }
        Ok(canonical_path)
    } else {
        if let Some(parent) = full_path.parent() {
            if parent.exists() {
                 let canonical_parent = parent.canonicalize().map_err(|e| FsError::Io(e.to_string()))?;
                 let canonical_base = base.canonicalize().map_err(|e| FsError::Io(e.to_string()))?;
                 if !canonical_parent.starts_with(&canonical_base) {
                      return Err(FsError::SecurityViolation);
                 }
            }
        }
        Ok(full_path)
    }
}

pub fn build_file_tree(root: &Path, current_dir: &Path) -> Result<Vec<FileEntry>, FsError> {
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(current_dir).map_err(|e| FsError::Io(e.to_string()))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| FsError::Io(e.to_string()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name == ".git" || name == "target" || name == "node_modules" || name == ".vscode" {
            continue;
        }

        let relative_path = path.strip_prefix(root)
            .map_err(|_| FsError::InvalidPath)?
            .to_string_lossy()
            .to_string();

        let is_dir = path.is_dir();
        let mut children = None;

        if is_dir {
            children = Some(build_file_tree(root, &path)?);
        }

        entries.push(FileEntry {
            path: relative_path,
            name,
            is_dir,
            children,
        });
    }
    entries.sort_by(|a, b| {
         if a.is_dir == b.is_dir {
             a.name.cmp(&b.name)
         } else {
             b.is_dir.cmp(&a.is_dir)
         }
    });

    Ok(entries)
}

pub fn search_code_internal(root: &Path, query: &str) -> Result<Vec<String>, FsError> {
    let matcher = RegexMatcher::new(query).map_err(|e| FsError::Io(format!("Regex error: {}", e)))?;
    let mut matches = Vec::new();
    let matches_mutex = std::sync::Mutex::new(&mut matches);

    WalkBuilder::new(root).build_parallel().run(|| {
        let mut searcher = Searcher::new();
        let matcher = matcher.clone();
        let matches_mutex = &matches_mutex; // Reference to mutex
        Box::new(move |result| {
            if let Ok(entry) = result {
                if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                     return ignore::WalkState::Continue;
                }

                let _ = searcher.search_path(&matcher, entry.path(), UTF8(|lnumm, line| {
                     let line_str = line.to_string();
                     // Format: path:line: content
                     let path_display = entry.path().strip_prefix(root).unwrap_or(entry.path()).to_string_lossy();
                     let match_entry = format!("{}:{}: {}", path_display, lnumm, line_str.trim());

                     if let Ok(mut lock) = matches_mutex.lock() {
                         lock.push(match_entry);
                     }

                     Ok(true) // Continue searching
                }));
            }
            ignore::WalkState::Continue
        })
    });

    Ok(matches)
}

fn validate_syntax(path: &str, content: &str) -> Result<(), String> {
    if path.ends_with(".rs") {
        parse_file(content).map_err(|e| format!("Rust Syntax Error: {}", e))?;
    } else if path.ends_with(".ts") || path.ends_with(".js") || path.ends_with(".tsx") || path.ends_with(".jsx") {
        let allocator = oxc_allocator::Allocator::default();
        let source_type = oxc_span::SourceType::from_path(std::path::Path::new(path)).unwrap_or_default();
        let ret = oxc_parser::Parser::new(&allocator, content, source_type).parse();

        if !ret.errors.is_empty() {
             return Err(format!("JS/TS Syntax Error: {:?}", ret.errors[0]));
        }
    }
    Ok(())
}

pub fn read_file_internal(root: &Path, file_path: String) -> Result<FileContent, FsError> {
    let full_path = validate_path(root, &file_path, true)?;
    let content = std::fs::read_to_string(&full_path).map_err(|e| FsError::Io(e.to_string()))?;
    Ok(FileContent {
        path: file_path,
        content
    })
}

pub fn write_file_internal(root: &Path, file_path: String, content: String) -> Result<FileContent, FsError> {
    let full_path = validate_path(root, &file_path, false)?;

    // Syntax Validation
    if let Err(e) = validate_syntax(&file_path, &content) {
        return Err(FsError::Syntax(e));
    }

    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| FsError::Io(e.to_string()))?;
    }

    std::fs::write(&full_path, content.clone()).map_err(|e| FsError::Io(e.to_string()))?;

    Ok(FileContent {
        path: file_path,
        content
    })
}

pub mod commands {
    use super::*;
    use tauri::State;

    #[tauri::command]
    #[specta::specta]
    pub async fn list_files(state: State<'_, WorkspaceState>, dir_path: Option<String>) -> Result<Vec<FileEntry>, FsError> {
        let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();
        let start_dir = if let Some(sub) = dir_path {
             validate_path(&root, &sub, true)?
        } else {
             root.clone()
        };
        build_file_tree(&root, &start_dir)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn read_file(state: State<'_, WorkspaceState>, file_path: String) -> Result<FileContent, FsError> {
        let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();
        read_file_internal(&root, file_path)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn write_file(state: State<'_, WorkspaceState>, file_path: String, content: String) -> Result<FileContent, FsError> {
         let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();
         write_file_internal(&root, file_path, content)
    }

    // NOTE: search_code not yet exposed to frontend via Tauri command in existing code,
    // but the Agent might use it via agent_core.
    // If frontend needs it, we can add it here.
    #[tauri::command]
    #[specta::specta]
    pub async fn search_code(state: State<'_, WorkspaceState>, query: String) -> Result<Vec<String>, FsError> {
         let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();
         search_code_internal(&root, &query)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn read_skeleton(state: State<'_, WorkspaceState>, file_path: String) -> Result<String, FsError> {
        let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();
        let fc = read_file_internal(&root, file_path.clone())?;
        get_skeleton(Path::new(&file_path), &fc.content).map_err(|e| FsError::Io(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_validate_path_security() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let outside_dir = tempdir().unwrap();
        let outside_file = outside_dir.path().join("secret.txt");
        File::create(&outside_file).unwrap();
        let res = validate_path(root, "../secret.txt", false);
        assert!(matches!(res, Err(FsError::SecurityViolation)));
        let inside_file = root.join("safe.txt");
        File::create(&inside_file).unwrap();
        let res = validate_path(root, "safe.txt", true);
        assert!(res.is_ok());
    }

    #[test]
    fn test_syntax_validation_rust() {
        let valid = "fn main() { println!(\"Hello\"); }";
        assert!(validate_syntax("main.rs", valid).is_ok());

        let invalid = "fn main() { println!(\"Hello\") "; // missing brace
        assert!(validate_syntax("main.rs", invalid).is_err());
    }

    #[test]
    fn test_syntax_validation_ts() {
        let valid = "const x: number = 10;";
        assert!(validate_syntax("test.ts", valid).is_ok());

        let invalid = "const x: number = ;";
        assert!(validate_syntax("test.ts", invalid).is_err());
    }
}
