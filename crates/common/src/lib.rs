use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct WorkspaceState(pub Arc<Mutex<PathBuf>>);
