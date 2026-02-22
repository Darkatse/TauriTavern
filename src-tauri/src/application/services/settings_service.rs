use serde_json::Value;
use std::sync::Arc;

use crate::application::dto::settings_dto::{
    AppSettingsDto, SettingsSnapshotDto, SillyTavernSettingsResponseDto, UpdateAppSettingsDto,
    UserSettingsDto,
};
use crate::application::errors::ApplicationError;
use crate::domain::repositories::settings_repository::SettingsRepository;

pub struct SettingsService {
    settings_repository: Arc<dyn SettingsRepository>,
}

impl SettingsService {
    pub fn new(settings_repository: Arc<dyn SettingsRepository>) -> Self {
        Self {
            settings_repository,
        }
    }

    pub async fn get_settings(&self) -> Result<AppSettingsDto, ApplicationError> {
        tracing::debug!("Getting application settings");

        let settings = self.settings_repository.load().await?;

        Ok(AppSettingsDto::from(settings))
    }

    pub async fn update_settings(
        &self,
        dto: UpdateAppSettingsDto,
    ) -> Result<AppSettingsDto, ApplicationError> {
        tracing::debug!("Updating application settings");

        let mut settings = self.settings_repository.load().await?;

        if let Some(server) = dto.server {
            settings.server.port = server.port;
            settings.server.host = server.host;
            settings.server.data_directory = server.data_directory;
        }

        if let Some(interface) = dto.interface {
            settings.interface.default_theme = interface.default_theme;
            settings.interface.default_character = interface.default_character;
            settings.interface.show_welcome_message = interface.show_welcome_message;
        }

        if let Some(security) = dto.security {
            settings.security.enable_authentication = security.enable_authentication;
            settings.security.session_timeout_minutes = security.session_timeout_minutes;
        }

        self.settings_repository.save(&settings).await?;

        Ok(AppSettingsDto::from(settings))
    }

    // SillyTavern 设置 API

    /// 保存用户设置
    pub async fn save_user_settings(
        &self,
        settings: UserSettingsDto,
    ) -> Result<(), ApplicationError> {
        tracing::info!("Saving user settings");

        let user_settings = settings.into();
        self.settings_repository
            .save_user_settings(&user_settings)
            .await?;

        Ok(())
    }

    /// 获取 SillyTavern 设置
    pub async fn get_sillytavern_settings(
        &self,
    ) -> Result<SillyTavernSettingsResponseDto, ApplicationError> {
        tracing::info!("Getting SillyTavern settings");

        // 加载用户设置
        let user_settings = self.settings_repository.load_user_settings().await?;
        let settings_json = serde_json::to_string(&user_settings.data).map_err(|e| {
            ApplicationError::InternalError(format!("Failed to serialize settings: {}", e))
        })?;

        // 获取 KoboldAI 设置
        let (koboldai_settings, koboldai_setting_names) =
            self.settings_repository.get_koboldai_settings().await?;

        // 获取 NovelAI 设置
        let (novelai_settings, novelai_setting_names) =
            self.settings_repository.get_novelai_settings().await?;

        // 获取 OpenAI 设置
        let (openai_settings, openai_setting_names) =
            self.settings_repository.get_openai_settings().await?;

        // 获取 TextGen 设置
        let (textgen_settings, textgen_setting_names) =
            self.settings_repository.get_textgen_settings().await?;

        // 获取世界名称
        let world_names = self.settings_repository.get_world_names().await?;

        // 获取主题
        let themes = self.settings_repository.get_themes().await?;
        let themes_json: Vec<Value> = themes.into_iter().map(|t| t.data).collect();

        // 获取 MovingUI 预设
        let moving_ui_presets = self.settings_repository.get_moving_ui_presets().await?;
        let moving_ui_presets_json: Vec<Value> =
            moving_ui_presets.into_iter().map(|p| p.data).collect();

        // 获取快速回复预设
        let quick_reply_presets = self.settings_repository.get_quick_reply_presets().await?;
        let quick_reply_presets_json: Vec<Value> =
            quick_reply_presets.into_iter().map(|p| p.data).collect();

        // 获取指令预设
        let instruct_presets = self.settings_repository.get_instruct_presets().await?;
        let instruct_presets_json: Vec<Value> =
            instruct_presets.into_iter().map(|p| p.data).collect();

        // 获取上下文预设
        let context_presets = self.settings_repository.get_context_presets().await?;
        let context_presets_json: Vec<Value> =
            context_presets.into_iter().map(|p| p.data).collect();

        // 获取系统提示预设
        let sysprompt_presets = self.settings_repository.get_sysprompt_presets().await?;
        let sysprompt_presets_json: Vec<Value> =
            sysprompt_presets.into_iter().map(|p| p.data).collect();

        // 获取推理预设
        let reasoning_presets = self.settings_repository.get_reasoning_presets().await?;
        let reasoning_presets_json: Vec<Value> =
            reasoning_presets.into_iter().map(|p| p.data).collect();

        // 构建响应
        let response = SillyTavernSettingsResponseDto {
            settings: settings_json,
            koboldai_settings,
            koboldai_setting_names,
            world_names,
            novelai_settings,
            novelai_setting_names,
            openai_settings,
            openai_setting_names,
            textgenerationwebui_presets: textgen_settings,
            textgenerationwebui_preset_names: textgen_setting_names,
            themes: themes_json,
            moving_ui_presets: moving_ui_presets_json,
            quick_reply_presets: quick_reply_presets_json,
            instruct: instruct_presets_json,
            context: context_presets_json,
            sysprompt: sysprompt_presets_json,
            reasoning: reasoning_presets_json,
            enable_extensions: true,             // 默认启用扩展
            enable_extensions_auto_update: true, // 默认启用扩展自动更新
            enable_accounts: false,              // 默认禁用账户
        };

        Ok(response)
    }

    /// 创建设置快照
    pub async fn create_snapshot(&self) -> Result<(), ApplicationError> {
        tracing::info!("Creating settings snapshot");

        self.settings_repository.create_snapshot().await?;

        Ok(())
    }

    /// 获取设置快照列表
    pub async fn get_snapshots(&self) -> Result<Vec<SettingsSnapshotDto>, ApplicationError> {
        tracing::info!("Getting settings snapshots");

        let snapshots = self.settings_repository.get_snapshots().await?;
        let snapshot_dtos = snapshots
            .into_iter()
            .map(SettingsSnapshotDto::from)
            .collect();

        Ok(snapshot_dtos)
    }

    /// 加载设置快照
    pub async fn load_snapshot(&self, name: &str) -> Result<UserSettingsDto, ApplicationError> {
        tracing::info!("Loading settings snapshot: {}", name);

        let settings = self.settings_repository.load_snapshot(name).await?;

        Ok(UserSettingsDto::from(settings))
    }

    /// 恢复设置快照
    pub async fn restore_snapshot(&self, name: &str) -> Result<(), ApplicationError> {
        tracing::info!("Restoring settings snapshot: {}", name);

        self.settings_repository.restore_snapshot(name).await?;

        Ok(())
    }
}
