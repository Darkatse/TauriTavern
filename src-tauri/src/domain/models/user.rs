use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub avatar: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub settings: UserSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    pub theme: String,
    pub interface: UserInterfaceSettings,
    pub generation: UserGenerationSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInterfaceSettings {
    pub avatar_style: String,
    pub chat_display: String,
    pub font_size: u8,
    pub blur_strength: u8,
    pub show_timestamps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserGenerationSettings {
    pub max_context_length: usize,
    pub response_length: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub repetition_penalty: f32,
}

impl User {
    pub fn new(username: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            username,
            avatar: None,
            created_at: now,
            updated_at: now,
            settings: UserSettings::default(),
        }
    }
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
            interface: UserInterfaceSettings::default(),
            generation: UserGenerationSettings::default(),
        }
    }
}

impl Default for UserInterfaceSettings {
    fn default() -> Self {
        Self {
            avatar_style: "round".to_string(),
            chat_display: "bubbles".to_string(),
            font_size: 16,
            blur_strength: 10,
            show_timestamps: true,
        }
    }
}

impl Default for UserGenerationSettings {
    fn default() -> Self {
        Self {
            max_context_length: 2048,
            response_length: 512,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repetition_penalty: 1.1,
        }
    }
}
