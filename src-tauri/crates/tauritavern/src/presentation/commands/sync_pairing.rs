use tauri::WebviewWindow;

use crate::presentation::commands::access_consent::{ConsentCopy, ask, localized};
use crate::presentation::errors::CommandError;
use tt_domain::errors::DomainError;

pub(super) enum PairingPeer<'a> {
    LanDevice {
        name: Option<&'a str>,
        host: &'a str,
    },
    SyncServer {
        origin: &'a str,
    },
}

pub(super) async fn ensure_pairing_consent(
    window: &WebviewWindow,
    locale: &str,
    peer: PairingPeer<'_>,
) -> Result<(), CommandError> {
    if ask_pairing_consent(window, locale, peer).await? {
        Ok(())
    } else {
        Err(CommandError::Cancelled(
            "Pairing cancelled by user".to_string(),
        ))
    }
}

pub(super) async fn ask_pairing_consent(
    window: &WebviewWindow,
    locale: &str,
    peer: PairingPeer<'_>,
) -> Result<bool, DomainError> {
    let (subject, copy) = match peer {
        PairingPeer::LanDevice { name, host } => (
            name.map_or_else(|| host.to_string(), |name| format!("{name} ({host})")),
            localized(locale, &LAN_ZH_HANS, &LAN_ZH_HANT, &LAN_EN),
        ),
        PairingPeer::SyncServer { origin } => (
            origin.to_string(),
            localized(locale, &SERVER_ZH_HANS, &SERVER_ZH_HANT, &SERVER_EN),
        ),
    };
    ask(window, Some(&subject), copy).await
}

static LAN_ZH_HANS: ConsentCopy = ConsentCopy {
    title: "允许与此设备配对？",
    subject_label: "设备：",
    warning: "仅当你正在配对这台设备时继续。配对后，在本机接收服务开启时，它可以读取本机数据，包括 API 密钥。",
    http_warning: None,
    confirm: "允许配对",
    cancel: "取消",
};

static LAN_ZH_HANT: ConsentCopy = ConsentCopy {
    title: "允許與此裝置配對？",
    subject_label: "裝置：",
    warning: "僅當你正在配對這台裝置時繼續。配對後，在本機接收服務開啟時，它可以讀取本機資料，包括 API 金鑰。",
    http_warning: None,
    confirm: "允許配對",
    cancel: "取消",
};

static LAN_EN: ConsentCopy = ConsentCopy {
    title: "Allow pairing with this device?",
    subject_label: "Device:",
    warning: "Continue only if you are pairing this device. Once paired, it can read this device's data, including API keys, while the receiver is on.",
    http_warning: None,
    confirm: "Allow Pairing",
    cancel: "Cancel",
};

static SERVER_ZH_HANS: ConsentCopy = ConsentCopy {
    title: "允许与 TT-Sync 服务器配对？",
    subject_label: "服务器：",
    warning: "第三方扩展也可能发起此请求。仅当这是你刚刚添加的服务器时继续。配对后，同步会把你选择的数据上传到它，可能包括 API 密钥。",
    http_warning: Some("\n此 HTTP 连接未加密，同步内容可能被观察或修改。"),
    confirm: "信任并配对",
    cancel: "取消",
};

static SERVER_ZH_HANT: ConsentCopy = ConsentCopy {
    title: "允許與 TT-Sync 伺服器配對？",
    subject_label: "伺服器：",
    warning: "第三方擴充功能也可能發起此請求。僅當這是你剛剛新增的伺服器時繼續。配對後，同步會把你選擇的資料上傳到它，可能包括 API 金鑰。",
    http_warning: Some("\n此 HTTP 連線未加密，同步內容可能被觀察或修改。"),
    confirm: "信任並配對",
    cancel: "取消",
};

static SERVER_EN: ConsentCopy = ConsentCopy {
    title: "Allow pairing with this TT-Sync server?",
    subject_label: "Server:",
    warning: "A third-party extension can also request this pairing. Continue only if you just added this server. Once paired, sync uploads the data you choose to it, which may include API keys.",
    http_warning: Some(
        "\nThis HTTP connection is unencrypted, so synced data may be observed or modified in transit.",
    ),
    confirm: "Trust & Pair",
    cancel: "Cancel",
};
