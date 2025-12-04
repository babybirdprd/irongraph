use specta::Type;

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Type)]
pub struct UserProfile {
    pub id: i32,
    pub name: String,
    pub bio: String,
}

#[derive(Clone)]
pub struct DbPool;

impl DbPool {
    pub fn new() -> Self {
        Self
    }

    // Mock method to simulate updating a user
    pub fn update_user(&self, name: String, bio: String) -> UserProfile {
        UserProfile {
            id: 1,
            name,
            bio,
        }
    }
}
