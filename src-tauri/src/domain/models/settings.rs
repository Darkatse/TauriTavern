use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 应用程序设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub server: ServerSettings,
    pub interface: InterfaceSettings,
    pub security: SecuritySettings,
}

/// 服务器设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    pub port: u16,
    pub host: String,
    pub data_directory: String,
}

/// 界面设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceSettings {
    pub default_theme: String,
    pub default_character: Option<String>,
    pub show_welcome_message: bool,
}

/// 安全设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySettings {
    pub enable_authentication: bool,
    pub session_timeout_minutes: u32,
}

/// SillyTavern 用户设置
/// 这是一个通用的设置结构，可以存储任何 JSON 数据
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    #[serde(flatten)]
    pub data: Value,
}

/// 设置快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub date: i64,
    pub name: String,
    pub size: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            server: ServerSettings::default(),
            interface: InterfaceSettings::default(),
            security: SecuritySettings::default(),
        }
    }
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            port: 8000,
            host: "127.0.0.1".to_string(),
            data_directory: "data".to_string(),
        }
    }
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        Self {
            default_theme: "default".to_string(),
            default_character: None,
            show_welcome_message: true,
        }
    }
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            enable_authentication: false,
            session_timeout_minutes: 60,
        }
    }
}
