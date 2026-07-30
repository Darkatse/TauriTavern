use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    Stable,
    Canary,
}

/// GitHub Release 中与更新相关的核心字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    /// Release tag，例如 "v2.1.1" 或 "Canary"。
    pub tag_name: String,
    /// Stable Release 的语义化版本号；Canary 使用 source_revision 判别。
    pub version: Option<String>,
    /// Canary tag 指向的 commit SHA；Stable 不需要该字段。
    pub source_revision: Option<String>,
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
    /// 本次检查使用的渠道。
    pub channel: UpdateChannel,
    /// 用于启动弹窗去重的稳定发布身份。
    pub release_token: Option<String>,
    /// 最新版本的 Release 信息，仅当 has_update 为 true 时有值。
    pub latest_release: Option<ReleaseInfo>,
}
