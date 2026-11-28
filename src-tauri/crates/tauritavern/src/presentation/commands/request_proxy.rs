use tauri::WebviewWindow;
use url::{Position, Url};

use crate::presentation::commands::access_consent::{ConsentCopy, ask, localized};
use crate::presentation::errors::CommandError;

pub(super) async fn ensure_request_proxy_consent(
    window: &WebviewWindow,
    locale: &str,
    proxy: &Url,
) -> Result<(), CommandError> {
    let copy = localized(locale, &ZH_HANS, &ZH_HANT, &EN);
    if ask(window, Some(&proxy_server(proxy)), copy).await? {
        Ok(())
    } else {
        Err(CommandError::Cancelled(
            "Request proxy change cancelled by user".to_string(),
        ))
    }
}

/// User info is left out: it can be written to look like a host.
fn proxy_server(proxy: &Url) -> String {
    format!(
        "{}://{}",
        proxy.scheme(),
        &proxy[Position::BeforeHost..Position::AfterPort]
    )
}

static ZH_HANS: ConsentCopy = ConsentCopy {
    title: "允许使用此出站代理？",
    subject_label: "代理：",
    warning: "第三方扩展也可能发起此更改。仅当这是你自己设置的代理时继续。除绕过列表中的主机外，TauriTavern 的网络请求都将经过它，未加密的 HTTP 请求可能被它观察或修改。",
    http_warning: None,
    confirm: "使用此代理",
    cancel: "取消",
};

static ZH_HANT: ConsentCopy = ConsentCopy {
    title: "允許使用此出站代理？",
    subject_label: "代理：",
    warning: "第三方擴充功能也可能發起此變更。僅當這是你自己設定的代理時繼續。除略過清單中的主機外，TauriTavern 的網路請求都將經過它，未加密的 HTTP 請求可能被它觀察或修改。",
    http_warning: None,
    confirm: "使用此代理",
    cancel: "取消",
};

static EN: ConsentCopy = ConsentCopy {
    title: "Use this request proxy?",
    subject_label: "Proxy:",
    warning: "A third-party extension can also make this change. Continue only if you set up this proxy yourself. TauriTavern's network requests, except to bypassed hosts, will go through it, and it may observe or modify unencrypted HTTP requests.",
    http_warning: None,
    confirm: "Use Proxy",
    cancel: "Cancel",
};

#[cfg(test)]
mod tests {
    use tt_domain::models::settings::RequestProxySettings;

    use super::proxy_server;

    #[test]
    fn dialog_names_the_proxy_host_without_user_info() {
        for (url, server) in [
            ("127.0.0.1:7890", "http://127.0.0.1:7890"),
            (
                "http://127.0.0.1:7890@proxy.example:8080",
                "http://proxy.example:8080",
            ),
            ("socks5h://user:secret@[::1]:1080", "socks5h://[::1]:1080"),
        ] {
            let proxy = RequestProxySettings {
                enabled: true,
                url: url.to_string(),
                bypass: vec![],
            }
            .proxy_url()
            .unwrap()
            .unwrap();
            assert_eq!(proxy_server(&proxy), server, "{url}");
        }
    }
}
