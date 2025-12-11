use tauri::State;
use shared_db::{DbPool, UserProfile};

pub fn update_profile_logic(state: &DbPool, name: String, bio: String) -> Result<UserProfile, String> {
    if name.is_empty() { return Err("Name required".into()); }
    let user = state.update_user(name, bio);
    Ok(user)
}
