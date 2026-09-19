use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::{Mutex, oneshot};

use crate::presentation::errors::CommandError;
use tt_application::services::user_endpoint_access_service::UserEndpointAccessService;

static DIALOG_GATE: Mutex<()> = Mutex::const_new(());

pub(super) async fn ensure_user_endpoint_access(
    endpoint: Option<String>,
    locale: &str,
    app_handle: &AppHandle,
    access_service: &UserEndpointAccessService,
) -> Result<(), CommandError> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };

    if access_service.is_granted(&endpoint).await {
        return Ok(());
    }

    let _guard = DIALOG_GATE.lock().await;
    if access_service.is_granted(&endpoint).await {
        return Ok(());
    }
    if !show_authorization_dialog(app_handle, &endpoint, locale).await? {
        return Err(CommandError::Cancelled(
            "Endpoint authorization cancelled by user".to_string(),
        ));
    }

    access_service.grant(endpoint).await;
    Ok(())
}

async fn show_authorization_dialog(
    app_handle: &AppHandle,
    endpoint: &str,
    locale: &str,
) -> Result<bool, CommandError> {
    let copy = dialog_copy(locale);
    let http_warning = if endpoint.starts_with("http://") {
        copy.http_warning
    } else {
        ""
    };
    let message = format!(
        "{} {}\n\n{}{}",
        copy.endpoint_label, endpoint, copy.warning, http_warning
    );

    let (sender, receiver) = oneshot::channel();
    app_handle
        .dialog()
        .message(message)
        .title(copy.title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            copy.confirm.to_string(),
            copy.cancel.to_string(),
        ))
        .show(move |confirmed| {
            let _ = sender.send(confirmed);
        });

    receiver.await.map_err(|_| {
        CommandError::InternalServerError(
            "Endpoint authorization dialog closed unexpectedly".to_string(),
        )
    })
}

struct DialogCopy {
    title: &'static str,
    endpoint_label: &'static str,
    warning: &'static str,
    http_warning: &'static str,
    confirm: &'static str,
    cancel: &'static str,
}

fn dialog_copy(locale: &str) -> DialogCopy {
    let locale = locale.trim().to_ascii_lowercase();
    if locale.starts_with("zh-cn") || locale.starts_with("zh-hans") {
        return DialogCopy {
            title: "允许连接到自定义端点？",
            endpoint_label: "端点：",
            warning: "第三方扩展也可能发起此请求。仅当这是你刚刚配置并认识的端点时继续；TauriTavern 可能向它发送 API 密钥、提示词和聊天内容。该端点后续将保持信任，请谨慎授权。",
            http_warning: "\n此 HTTP 连接未加密，传输内容可能被观察或修改。",
            confirm: "信任并连接",
            cancel: "取消",
        };
    }
    if locale.starts_with("zh-tw") || locale.starts_with("zh-hant") {
        return DialogCopy {
            title: "允許連線到自訂端點？",
            endpoint_label: "端點：",
            warning: "第三方擴充功能也可能發起此請求。僅當這是你剛剛設定並認識的端點時繼續；TauriTavern 可能向它傳送 API 金鑰、提示詞和聊天內容。該端點後續將保持信任，請謹慎授權。",
            http_warning: "\n此 HTTP 連線未加密，傳輸內容可能被觀察或修改。",
            confirm: "信任並連線",
            cancel: "取消",
        };
    }

    DialogCopy {
        title: "Allow custom endpoint?",
        endpoint_label: "Endpoint:",
        warning: "A third-party extension can also request this connection. Continue only if you just configured and recognize this endpoint. TauriTavern may send it API keys, prompts, and chat content. This endpoint will remain trusted; authorize it carefully.",
        http_warning: "\nThis HTTP connection is unencrypted and may be observed or modified in transit.",
        confirm: "Trust & Connect",
        cancel: "Cancel",
    }
}
