use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};
use thiserror::Error;
pub use common::WorkspaceState;

#[derive(Error, Debug, Serialize, Type)]
pub enum FsError {
    #[error("IO Error: {0}")]
    Io(String),
    #[error("Security Violation: Path traversal detected")]
    SecurityViolation,
    #[error("Invalid Path")]
    InvalidPath,
}

// Map std::io::Error to FsError
impl From<std::io::Error> for FsError {
    fn from(e: std::io::Error) -> Self {
        FsError::Io(e.to_string())
    }
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub path: String, // Relative path from workspace root
    pub name: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileEntry>>,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
pub struct FileContent {
    pub path: String,
    pub content: String,
}

// Security Helper
// Returns the absolute path if valid.
fn validate_path(base: &Path, user_path: &str, require_exists: bool) -> Result<PathBuf, FsError> {
    // 1. Basic lexical check for ".." in user_path (Strict Sandbox)
    let path_parts = Path::new(user_path);
    for component in path_parts.components() {
        if let std::path::Component::ParentDir = component {
            return Err(FsError::SecurityViolation);
        }
    }

    let full_path = base.join(user_path);

    if require_exists {
        // For reading: Canonicalize and check prefix
        let canonical_path = full_path.canonicalize().map_err(|e| FsError::Io(e.to_string()))?;
        let canonical_base = base.canonicalize().map_err(|e| FsError::Io(e.to_string()))?;

        if !canonical_path.starts_with(&canonical_base) {
             return Err(FsError::SecurityViolation);
        }
        Ok(canonical_path)
    } else {
        // For writing (file might not exist):
        // We already checked for ".." in user_path.
        // We assume base is safe.
        // So base.join(user_path) should be safe.
        // But to be extra safe, we can try to canonicalize the parent.
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

pub mod commands {
    use super::*;
    use tauri::State;

    #[tauri::command]
    #[specta::specta]
    pub async fn list_files(state: State<'_, WorkspaceState>, dir_path: Option<String>) -> Result<Vec<FileEntry>, FsError> {
        let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();

        // Determine start directory
        let start_dir = if let Some(sub) = dir_path {
             validate_path(&root, &sub, true)?
        } else {
             root.clone()
        };

        build_file_tree(&root, &start_dir)
    }

    fn build_file_tree(root: &Path, current_dir: &Path) -> Result<Vec<FileEntry>, FsError> {
        let mut entries = Vec::new();
        // If directory doesn't exist, return error (or empty?) - strict error
        let read_dir = std::fs::read_dir(current_dir).map_err(|e| FsError::Io(e.to_string()))?;

        for entry in read_dir {
            let entry = entry.map_err(|e| FsError::Io(e.to_string()))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Filter
            if name == ".git" || name == "target" || name == "node_modules" || name == ".vscode" {
                continue;
            }

            // Calculate relative path from ROOT (not current_dir)
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
        // Sort entries: directories first, then files
        entries.sort_by(|a, b| {
             if a.is_dir == b.is_dir {
                 a.name.cmp(&b.name)
             } else {
                 b.is_dir.cmp(&a.is_dir) // true > false
             }
        });

        Ok(entries)
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn read_file(state: State<'_, WorkspaceState>, file_path: String) -> Result<FileContent, FsError> {
        let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();
        let full_path = validate_path(&root, &file_path, true)?;

        let content = std::fs::read_to_string(&full_path).map_err(|e| FsError::Io(e.to_string()))?;

        Ok(FileContent {
            path: file_path,
            content
        })
    }

    #[tauri::command]
    #[specta::specta]
    pub async fn write_file(state: State<'_, WorkspaceState>, file_path: String, content: String) -> Result<FileContent, FsError> {
         let root = state.0.lock().map_err(|_| FsError::Io("Lock poison".into()))?.clone();
         let full_path = validate_path(&root, &file_path, false)?;

         if let Some(parent) = full_path.parent() {
             std::fs::create_dir_all(parent).map_err(|e| FsError::Io(e.to_string()))?;
         }

         std::fs::write(&full_path, content.clone()).map_err(|e| FsError::Io(e.to_string()))?;

         Ok(FileContent {
             path: file_path,
             content
         })
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

        // Create a file outside root
        let outside_dir = tempdir().unwrap();
        let outside_file = outside_dir.path().join("secret.txt");
        File::create(&outside_file).unwrap();

        // Attempt traversal
        let res = validate_path(root, "../secret.txt", false);
        assert!(matches!(res, Err(FsError::SecurityViolation)), "Should catch .. traversal");

        // Attempt normal file
        let inside_file = root.join("safe.txt");
        File::create(&inside_file).unwrap();
        let res = validate_path(root, "safe.txt", true);
        assert!(res.is_ok());
    }
}
