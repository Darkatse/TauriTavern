//! Consent dialogs never overlap, and a grant made while one was open is honored without asking
//! again.

use tauri::WebviewWindow;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::{Mutex, oneshot};
use tt_application::services::user_grants::UserGrants;
use tt_domain::errors::DomainError;

static DIALOG_GATE: Mutex<()> = Mutex::const_new(());

pub(super) struct ConsentCopy {
    pub(super) title: &'static str,
    pub(super) subject_label: &'static str,
    pub(super) warning: &'static str,
    pub(super) http_warning: Option<&'static str>,
    pub(super) confirm: &'static str,
    pub(super) cancel: &'static str,
}

pub(super) fn localized(
    locale: &str,
    zh_hans: &'static ConsentCopy,
    zh_hant: &'static ConsentCopy,
    en: &'static ConsentCopy,
) -> &'static ConsentCopy {
    let locale = locale.trim().to_ascii_lowercase();
    if locale.starts_with("zh-cn") || locale.starts_with("zh-hans") {
        zh_hans
    } else if locale.starts_with("zh-tw") || locale.starts_with("zh-hant") {
        zh_hant
    } else {
        en
    }
}

pub(super) async fn ask(
    window: &WebviewWindow,
    subject: Option<&str>,
    copy: &ConsentCopy,
) -> Result<bool, DomainError> {
    let _turn = DIALOG_GATE.lock().await;
    show_dialog(window, subject, copy).await
}

pub(super) async fn ask_once(
    window: &WebviewWindow,
    grants: &dyn UserGrants,
    subject: &str,
    copy: &ConsentCopy,
    cancelled: &'static str,
) -> Result<(), DomainError> {
    if grants.is_allowed(subject).await {
        return Ok(());
    }

    let _turn = DIALOG_GATE.lock().await;
    if grants.is_allowed(subject).await {
        return Ok(());
    }
    if !show_dialog(window, Some(subject), copy).await? {
        return Err(DomainError::Cancelled(cancelled.to_string()));
    }

    grants.grant(subject).await;
    Ok(())
}

async fn show_dialog(
    window: &WebviewWindow,
    subject: Option<&str>,
    copy: &ConsentCopy,
) -> Result<bool, DomainError> {
    let message = match subject {
        Some(subject) => {
            let http_warning = copy
                .http_warning
                .filter(|_| subject.starts_with("http://"))
                .unwrap_or_default();
            format!(
                "{} {subject}\n\n{}{http_warning}",
                copy.subject_label, copy.warning
            )
        }
        None => copy.warning.to_string(),
    };

    let dialog = window.dialog().message(message);
    // Without a parent the sheet goes to `NSApp.mainWindow`, which is unset while TauriTavern is
    // inactive, and never appears.
    #[cfg(desktop)]
    let dialog = dialog.parent(window);

    let (sender, receiver) = oneshot::channel();
    dialog
        .title(copy.title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            copy.confirm.to_string(),
            copy.cancel.to_string(),
        ))
        .show(move |confirmed| {
            let _ = sender.send(confirmed);
        });

    receiver
        .await
        .map_err(|_| DomainError::InternalError("Consent dialog closed unexpectedly".to_string()))
}
