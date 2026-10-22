//! WebView transfer budgets. Receivers consume these values without knowing the OS.

use super::identity::HOST_IDENTITY;
use tt_contracts::host::HostPlatform;

pub const COMMIT_FRAME_BYTES: u64 = match HOST_IDENTITY.platform {
    HostPlatform::Android => 256 * 1024,
    HostPlatform::Ios => 1024 * 1024,
    HostPlatform::Windows | HostPlatform::Macos | HostPlatform::Linux | HostPlatform::Ohos => {
        4 * 1024 * 1024
    }
};

pub const UPLOAD_CHUNK_BYTES: u64 = match HOST_IDENTITY.platform {
    HostPlatform::Android | HostPlatform::Ios => 1024 * 1024,
    HostPlatform::Windows | HostPlatform::Macos | HostPlatform::Linux | HostPlatform::Ohos => {
        4 * 1024 * 1024
    }
};

pub const SMALL_ASSET_UPLOAD_CHUNK_BYTES: u64 = match HOST_IDENTITY.platform {
    HostPlatform::Android | HostPlatform::Ios => 512 * 1024,
    HostPlatform::Windows | HostPlatform::Macos | HostPlatform::Linux | HostPlatform::Ohos => {
        4 * 1024 * 1024
    }
};
