//! Every import download gets its `ImportSources` here; each URL it requests must be pre-approved
//! or confirmed once by the user.

use async_trait::async_trait;
use tauri::WebviewWindow;
use url::Url;

use crate::app::AppState;
use crate::presentation::commands::access_consent::{
    ConsentCopy, ConsentRequest, ask_once, localized,
};
use crate::presentation::commands::helpers::ensure_ios_policy_allows;
use crate::presentation::errors::CommandError;
use tt_application::services::external_import_service::{
    DownloadSources, ImportSourceAccessService, import_source,
};
use tt_domain::errors::DomainError;

pub(super) struct ImportSources<'a> {
    window: &'a WebviewWindow,
    locale: &'a str,
    access: &'a ImportSourceAccessService,
}

impl<'a> ImportSources<'a> {
    pub(super) fn new(
        window: &'a WebviewWindow,
        app_state: &'a AppState,
        locale: &'a str,
    ) -> Result<Self, CommandError> {
        ensure_ios_policy_allows(
            &app_state.ios_policy,
            app_state.ios_policy.capabilities.content.external_import,
            "content.external_import",
        )?;
        Ok(Self {
            window,
            locale,
            access: &app_state.services.import_source_access_service,
        })
    }
}

#[async_trait]
impl DownloadSources for ImportSources<'_> {
    async fn allow(&self, url: &Url) -> Result<(), DomainError> {
        let source = import_source(url);
        let request = ConsentRequest {
            subject: &source,
            copy: localized(self.locale, &ZH_HANS, &ZH_HANT, &EN),
            cancelled: "Import source authorization cancelled by user",
        };
        ask_once(self.window, self.access, request).await
    }
}

static ZH_HANS: ConsentCopy = ConsentCopy {
    title: "允许从此来源导入？",
    subject_label: "来源：",
    warning: "第三方扩展也可能发起此请求。仅当你正在从这里导入时继续。授权后，从该来源导入将不再询问。",
    http_warning: "\n此 HTTP 连接未加密，下载内容可能被观察或修改。",
    confirm: "允许导入",
    cancel: "取消",
};

static ZH_HANT: ConsentCopy = ConsentCopy {
    title: "允許從此來源匯入？",
    subject_label: "來源：",
    warning: "第三方擴充功能也可能發起此請求。僅當你正在從這裡匯入時繼續。授權後，從該來源匯入將不再詢問。",
    http_warning: "\n此 HTTP 連線未加密，下載內容可能被觀察或修改。",
    confirm: "允許匯入",
    cancel: "取消",
};

static EN: ConsentCopy = ConsentCopy {
    title: "Allow importing from this source?",
    subject_label: "Source:",
    warning: "A third-party extension can also request this download. Continue only if you are importing from here. Imports from this source will not ask again.",
    http_warning: "\nThis HTTP connection is unencrypted, so the download may be observed or modified in transit.",
    confirm: "Allow Import",
    cancel: "Cancel",
};
