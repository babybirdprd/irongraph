use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::mpsc;
use portable_pty::{Child};
use std::collections::HashMap;
use std::io::{Write};
use radkit::tools::ExecutionState;
use serde_json::Value;

pub struct PtySession {
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send + Sync>,
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub struct TerminalState {
    pub sessions: Mutex<HashMap<String, Arc<Mutex<PtySession>>>>,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

// Global Registry for Heavy State
pub static SESSION_REGISTRY: OnceLock<Mutex<HashMap<String, Arc<RadkitState>>>> = OnceLock::new();

pub fn register_session(id: String, state: Arc<RadkitState>) {
    let registry = SESSION_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
    registry.lock().unwrap().insert(id, state);
}

pub fn unregister_session(id: &str) {
    if let Some(registry) = SESSION_REGISTRY.get() {
        registry.lock().unwrap().remove(id);
    }
}

pub fn get_session(id: &str) -> Option<Arc<RadkitState>> {
     let registry = SESSION_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
     registry.lock().unwrap().get(id).cloned()
}

// Heavy State (Not passed to Radkit directly)
pub struct RadkitState {
    pub root: PathBuf,
    pub terminal_state: Arc<TerminalState>,
    pub session_id: String,
    pub command_buffer: Arc<Mutex<Option<mpsc::Sender<String>>>>,
}

// Lightweight JSON State (Passed to Radkit)
pub struct SessionState {
    pub store: Mutex<HashMap<String, Value>>,
}

impl SessionState {
    pub fn new(session_id: String) -> Self {
        let mut map = HashMap::new();
        map.insert("session_id".to_string(), Value::String(session_id));
        Self {
            store: Mutex::new(map)
        }
    }
}

impl ExecutionState for SessionState {
    fn set_state(&self, key: &str, value: Value) {
        let mut store = self.store.lock().unwrap();
        store.insert(key.to_string(), value);
    }

    fn get_state(&self, key: &str) -> Option<Value> {
        let store = self.store.lock().unwrap();
        store.get(key).cloned()
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkspaceState(pub Arc<Mutex<PathBuf>>);
