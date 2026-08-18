use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::{Mutex, oneshot};

use crate::app::AppState;
use crate::presentation::commands::helpers::log_command;
use crate::presentation::errors::CommandError;
use tt_application::dto::chat_completion_dto::ChatCompletionEndpointAccessRequestDto;
use tt_ports::local_endpoint_access::LocalEndpointCandidate;

static DIALOG_GATE: Mutex<()> = Mutex::const_new(());

#[tauri::command]
pub async fn authorize_chat_completion_endpoint(
    dto: ChatCompletionEndpointAccessRequestDto,
    locale: String,
    prompt: bool,
    app_handle: AppHandle,
    app_state: State<'_, Arc<AppState>>,
) -> Result<bool, CommandError> {
    log_command("authorize_chat_completion_endpoint");

    let endpoint = app_state
        .services
        .chat_completion_service
        .resolve_user_endpoint_for_access(&dto)?;
    let Some(endpoint) = endpoint else {
        return Ok(true);
    };

    // `prompt` suppresses automatic reconnect dialogs; native confirmation remains
    // the authorization boundary and cannot be supplied by the caller.
    let _dialog_guard = if prompt {
        Some(DIALOG_GATE.lock().await)
    } else {
        None
    };
    let candidate = app_state
        .services
        .local_endpoint_access_service
        .authorization_candidate(&endpoint)
        .await?;
    let Some(candidate) = candidate else {
        return Ok(true);
    };

    if !prompt {
        return Ok(false);
    }
    if !show_authorization_dialog(&app_handle, &candidate, &locale).await? {
        return Ok(false);
    }

    app_state
        .services
        .local_endpoint_access_service
        .grant(candidate.endpoint)
        .await;
    Ok(true)
}

async fn show_authorization_dialog(
    app_handle: &AppHandle,
    candidate: &LocalEndpointCandidate,
    locale: &str,
) -> Result<bool, CommandError> {
    let copy = dialog_copy(locale);
    let addresses = candidate.addresses.join(", ");
    let http_warning = if candidate.endpoint.starts_with("http://") {
        copy.http_warning
    } else {
        ""
    };
    let message = format!(
        "{} {}\n{} {}\n\n{}{}",
        copy.endpoint_label,
        candidate.endpoint,
        copy.addresses_label,
        addresses,
        copy.warning,
        http_warning,
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
            "Local endpoint authorization dialog closed unexpectedly".to_string(),
        )
    })
}

struct DialogCopy {
    title: &'static str,
    endpoint_label: &'static str,
    addresses_label: &'static str,
    warning: &'static str,
    http_warning: &'static str,
    confirm: &'static str,
    cancel: &'static str,
}

fn dialog_copy(locale: &str) -> DialogCopy {
    let locale = locale.trim().to_ascii_lowercase();
    if locale.starts_with("zh-cn") || locale.starts_with("zh-hans") {
        return DialogCopy {
            title: "允许连接到本地网络端点？",
            endpoint_label: "端点：",
            addresses_label: "地址：",
            warning: "第三方扩展也可能发起此请求。仅当这是你刚刚配置并认识的端点时继续；TauriTavern 可能向它发送 API 密钥、提示词和聊天内容。该端点将绕过 Request Proxy 直接连接，并在本次安装中保持信任。",
            http_warning: "\n此 HTTP 连接未加密，同一网络中的其他设备可能观察或修改传输内容。",
            confirm: "信任并连接",
            cancel: "取消",
        };
    }
    if locale.starts_with("zh-tw") || locale.starts_with("zh-hant") {
        return DialogCopy {
            title: "允許連線到區域網路端點？",
            endpoint_label: "端點：",
            addresses_label: "位址：",
            warning: "第三方擴充功能也可能發起此請求。僅當這是你剛剛設定並認識的端點時繼續；TauriTavern 可能向它傳送 API 金鑰、提示詞和聊天內容。該端點將略過 Request Proxy 直接連線，並在本次安裝中保持信任。",
            http_warning: "\n此 HTTP 連線未加密，同一網路中的其他裝置可能觀察或修改傳輸內容。",
            confirm: "信任並連線",
            cancel: "取消",
        };
    }

    DialogCopy {
        title: "Allow local network endpoint?",
        endpoint_label: "Endpoint:",
        addresses_label: "Addresses:",
        warning: "A third-party extension can also request this connection. Continue only if you just configured and recognize this endpoint. TauriTavern may send it API keys, prompts, and chat content. It will connect directly, bypassing Request Proxy, and remain trusted for this installation.",
        http_warning: "\nThis HTTP connection is unencrypted; other devices on the network may observe or modify its traffic.",
        confirm: "Trust & Connect",
        cancel: "Cancel",
    }
}
