use std::path::PathBuf;
use std::sync::Mutex;

pub struct WorkspaceState(pub Mutex<PathBuf>);
