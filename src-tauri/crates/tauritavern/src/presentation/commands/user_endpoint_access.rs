use tauri::WebviewWindow;

use crate::presentation::commands::access_consent::{
    ConsentCopy, ConsentRequest, ask_once, localized,
};
use crate::presentation::errors::CommandError;
use tt_application::services::user_endpoint_access_service::UserEndpointAccessService;

pub(super) async fn ensure_user_endpoint_access(
    endpoint: Option<String>,
    locale: &str,
    window: &WebviewWindow,
    access_service: &UserEndpointAccessService,
) -> Result<(), CommandError> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };

    let request = ConsentRequest {
        subject: &endpoint,
        copy: localized(locale, &ZH_HANS, &ZH_HANT, &EN),
        cancelled: "Endpoint authorization cancelled by user",
    };
    ask_once(window, access_service, request).await?;
    Ok(())
}

static ZH_HANS: ConsentCopy = ConsentCopy {
    title: "允许连接到自定义端点？",
    subject_label: "端点：",
    warning: "第三方扩展也可能发起此请求。仅当这是你刚刚配置并认识的端点时继续；TauriTavern 可能向它发送 API 密钥、提示词和聊天内容。该端点后续将保持信任，请谨慎授权。",
    http_warning: "\n此 HTTP 连接未加密，传输内容可能被观察或修改。",
    confirm: "信任并连接",
    cancel: "取消",
};

static ZH_HANT: ConsentCopy = ConsentCopy {
    title: "允許連線到自訂端點？",
    subject_label: "端點：",
    warning: "第三方擴充功能也可能發起此請求。僅當這是你剛剛設定並認識的端點時繼續；TauriTavern 可能向它傳送 API 金鑰、提示詞和聊天內容。該端點後續將保持信任，請謹慎授權。",
    http_warning: "\n此 HTTP 連線未加密，傳輸內容可能被觀察或修改。",
    confirm: "信任並連線",
    cancel: "取消",
};

static EN: ConsentCopy = ConsentCopy {
    title: "Allow custom endpoint?",
    subject_label: "Endpoint:",
    warning: "A third-party extension can also request this connection. Continue only if you just configured and recognize this endpoint. TauriTavern may send it API keys, prompts, and chat content. This endpoint will remain trusted; authorize it carefully.",
    http_warning: "\nThis HTTP connection is unencrypted and may be observed or modified in transit.",
    confirm: "Trust & Connect",
    cancel: "Cancel",
};
