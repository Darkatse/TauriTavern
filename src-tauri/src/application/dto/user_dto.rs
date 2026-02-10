use crate::domain::models::user::{
    User, UserGenerationSettings, UserInterfaceSettings, UserSettings,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDto {
    pub id: String,
    pub username: String,
    pub avatar: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub settings: UserSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettingsDto {
    pub theme: String,
    pub interface: UserInterfaceSettingsDto,
    pub generation: UserGenerationSettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInterfaceSettingsDto {
    pub avatar_style: String,
    pub chat_display: String,
    pub font_size: u8,
    pub blur_strength: u8,
    pub show_timestamps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserGenerationSettingsDto {
    pub max_context_length: usize,
    pub response_length: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub repetition_penalty: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserDto {
    pub username: String,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserDto {
    pub id: String,
    pub username: Option<String>,
    pub avatar: Option<String>,
    pub settings: Option<UserSettingsDto>,
}

impl From<User> for UserDto {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            avatar: user.avatar,
            created_at: user.created_at.to_rfc3339(),
            updated_at: user.updated_at.to_rfc3339(),
            settings: UserSettingsDto::from(user.settings),
        }
    }
}

impl From<UserSettings> for UserSettingsDto {
    fn from(settings: UserSettings) -> Self {
        Self {
            theme: settings.theme,
            interface: UserInterfaceSettingsDto::from(settings.interface),
            generation: UserGenerationSettingsDto::from(settings.generation),
        }
    }
}

impl From<UserInterfaceSettings> for UserInterfaceSettingsDto {
    fn from(settings: UserInterfaceSettings) -> Self {
        Self {
            avatar_style: settings.avatar_style,
            chat_display: settings.chat_display,
            font_size: settings.font_size,
            blur_strength: settings.blur_strength,
            show_timestamps: settings.show_timestamps,
        }
    }
}

impl From<UserGenerationSettings> for UserGenerationSettingsDto {
    fn from(settings: UserGenerationSettings) -> Self {
        Self {
            max_context_length: settings.max_context_length,
            response_length: settings.response_length,
            temperature: settings.temperature,
            top_p: settings.top_p,
            top_k: settings.top_k,
            repetition_penalty: settings.repetition_penalty,
        }
    }
}

impl From<UserSettingsDto> for UserSettings {
    fn from(dto: UserSettingsDto) -> Self {
        Self {
            theme: dto.theme,
            interface: UserInterfaceSettings::from(dto.interface),
            generation: UserGenerationSettings::from(dto.generation),
        }
    }
}

impl From<UserInterfaceSettingsDto> for UserInterfaceSettings {
    fn from(dto: UserInterfaceSettingsDto) -> Self {
        Self {
            avatar_style: dto.avatar_style,
            chat_display: dto.chat_display,
            font_size: dto.font_size,
            blur_strength: dto.blur_strength,
            show_timestamps: dto.show_timestamps,
        }
    }
}

impl From<UserGenerationSettingsDto> for UserGenerationSettings {
    fn from(dto: UserGenerationSettingsDto) -> Self {
        Self {
            max_context_length: dto.max_context_length,
            response_length: dto.response_length,
            temperature: dto.temperature,
            top_p: dto.top_p,
            top_k: dto.top_k,
            repetition_penalty: dto.repetition_penalty,
        }
    }
}
