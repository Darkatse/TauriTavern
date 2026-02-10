use crate::domain::models::settings::{
    AppSettings, InterfaceSettings, SecuritySettings, ServerSettings, SettingsSnapshot,
    UserSettings,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsDto {
    pub server: ServerSettingsDto,
    pub interface: InterfaceSettingsDto,
    pub security: SecuritySettingsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettingsDto {
    pub port: u16,
    pub host: String,
    pub data_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceSettingsDto {
    pub default_theme: String,
    pub default_character: Option<String>,
    pub show_welcome_message: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySettingsDto {
    pub enable_authentication: bool,
    pub session_timeout_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAppSettingsDto {
    pub server: Option<ServerSettingsDto>,
    pub interface: Option<InterfaceSettingsDto>,
    pub security: Option<SecuritySettingsDto>,
}

/// SillyTavern 用户设置 DTO
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettingsDto {
    #[serde(flatten)]
    pub data: Value,
}

/// 设置快照 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshotDto {
    pub date: i64,
    pub name: String,
    pub size: u64,
}

/// SillyTavern 设置响应 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SillyTavernSettingsResponseDto {
    pub settings: String,
    pub koboldai_settings: Vec<String>,
    pub koboldai_setting_names: Vec<String>,
    pub world_names: Vec<String>,
    pub novelai_settings: Vec<String>,
    pub novelai_setting_names: Vec<String>,
    pub openai_settings: Vec<String>,
    pub openai_setting_names: Vec<String>,
    pub textgenerationwebui_presets: Vec<String>,
    pub textgenerationwebui_preset_names: Vec<String>,
    pub themes: Vec<Value>,
    pub movingUIPresets: Vec<Value>,
    pub quickReplyPresets: Vec<Value>,
    pub instruct: Vec<Value>,
    pub context: Vec<Value>,
    pub sysprompt: Vec<Value>,
    pub reasoning: Vec<Value>,
    pub enable_extensions: bool,
    pub enable_extensions_auto_update: bool,
    pub enable_accounts: bool,
}

impl From<UserSettings> for UserSettingsDto {
    fn from(settings: UserSettings) -> Self {
        Self {
            data: settings.data,
        }
    }
}

impl From<UserSettingsDto> for UserSettings {
    fn from(dto: UserSettingsDto) -> Self {
        Self { data: dto.data }
    }
}

impl From<SettingsSnapshot> for SettingsSnapshotDto {
    fn from(snapshot: SettingsSnapshot) -> Self {
        Self {
            date: snapshot.date,
            name: snapshot.name,
            size: snapshot.size,
        }
    }
}

impl From<AppSettings> for AppSettingsDto {
    fn from(settings: AppSettings) -> Self {
        Self {
            server: ServerSettingsDto::from(settings.server),
            interface: InterfaceSettingsDto::from(settings.interface),
            security: SecuritySettingsDto::from(settings.security),
        }
    }
}

impl From<ServerSettings> for ServerSettingsDto {
    fn from(settings: ServerSettings) -> Self {
        Self {
            port: settings.port,
            host: settings.host,
            data_directory: settings.data_directory,
        }
    }
}

impl From<InterfaceSettings> for InterfaceSettingsDto {
    fn from(settings: InterfaceSettings) -> Self {
        Self {
            default_theme: settings.default_theme,
            default_character: settings.default_character,
            show_welcome_message: settings.show_welcome_message,
        }
    }
}

impl From<SecuritySettings> for SecuritySettingsDto {
    fn from(settings: SecuritySettings) -> Self {
        Self {
            enable_authentication: settings.enable_authentication,
            session_timeout_minutes: settings.session_timeout_minutes,
        }
    }
}

impl From<AppSettingsDto> for AppSettings {
    fn from(dto: AppSettingsDto) -> Self {
        Self {
            server: ServerSettings::from(dto.server),
            interface: InterfaceSettings::from(dto.interface),
            security: SecuritySettings::from(dto.security),
        }
    }
}

impl From<ServerSettingsDto> for ServerSettings {
    fn from(dto: ServerSettingsDto) -> Self {
        Self {
            port: dto.port,
            host: dto.host,
            data_directory: dto.data_directory,
        }
    }
}

impl From<InterfaceSettingsDto> for InterfaceSettings {
    fn from(dto: InterfaceSettingsDto) -> Self {
        Self {
            default_theme: dto.default_theme,
            default_character: dto.default_character,
            show_welcome_message: dto.show_welcome_message,
        }
    }
}

impl From<SecuritySettingsDto> for SecuritySettings {
    fn from(dto: SecuritySettingsDto) -> Self {
        Self {
            enable_authentication: dto.enable_authentication,
            session_timeout_minutes: dto.session_timeout_minutes,
        }
    }
}
