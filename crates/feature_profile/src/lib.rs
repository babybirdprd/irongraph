use specta::Type;
use tauri::State;
use shared_db::{DbPool, UserProfile};

#[derive(Type, serde::Deserialize, Debug)]
pub struct UpdateProfileReq {
    pub name: String,
    pub bio: String,
}

pub mod commands {
    use super::*;

    #[tauri::command]
    #[specta::specta]
    pub fn update_profile(state: State<DbPool>, req: UpdateProfileReq) -> Result<UserProfile, String> {
        if req.name.is_empty() { return Err("Name required".into()); }
        let user = state.update_user(req.name, req.bio);
        Ok(user)
    }
}
