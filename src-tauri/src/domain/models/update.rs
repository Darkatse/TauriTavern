use serde::{Deserialize, Serialize};

/// GitHub Release 中与更新相关的核心字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    /// Release tag，例如 "desktop-auto-v1.4.0"。
    pub tag_name: String,
    /// 语义化版本号，从 tag 中解析，例如 "1.4.0"。
    pub version: String,
    /// Release 标题。
    pub name: String,
    /// Release body（Markdown 变更日志）。
    pub body: String,
    /// GitHub Release 页面 URL。
    pub html_url: String,
    /// 是否为预发布。
    pub prerelease: bool,
    /// 发布时间（ISO 8601）。
    pub published_at: String,
}

/// 更新检查结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    /// 是否有可用更新。
    pub has_update: bool,
    /// 当前版本。
    pub current_version: String,
    /// 最新版本的 Release 信息，仅当 has_update 为 true 时有值。
    pub latest_release: Option<ReleaseInfo>,
}

