use serde::Serialize;

/// The native application target, independent of viewport and device shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostPlatform {
    Windows,
    Macos,
    Linux,
    Android,
    Ios,
    Ohos,
}

impl HostPlatform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Ohos => "ohos",
        }
    }
}

/// Shell category used by the native window and command implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Desktop,
    Mobile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HostIdentity {
    pub platform: HostPlatform,
    pub kind: HostKind,
}

pub const IOS_EXPORT_STAGING_ROOT_NAME: &str = "tauritavern-export-staging";
