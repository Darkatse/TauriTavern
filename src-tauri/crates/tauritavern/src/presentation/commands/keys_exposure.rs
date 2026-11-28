use tauri::WebviewWindow;

use crate::presentation::commands::access_consent::{ConsentCopy, ask, localized};
use crate::presentation::errors::CommandError;

pub(super) async fn ensure_keys_exposure_consent(
    window: &WebviewWindow,
    locale: &str,
) -> Result<(), CommandError> {
    let copy = localized(locale, &ZH_HANS, &ZH_HANT, &EN);
    if ask(window, None, copy).await? {
        Ok(())
    } else {
        Err(CommandError::Cancelled(
            "Keys exposure cancelled by user".to_string(),
        ))
    }
}

static ZH_HANS: ConsentCopy = ConsentCopy {
    title: "允许显示密钥？",
    subject_label: "",
    warning: "开启后，第三方扩展和角色卡脚本也能读取全部 API 密钥。只在需要查看或复制时开启，用完后关闭。重启应用后生效。",
    http_warning: None,
    confirm: "允许显示",
    cancel: "取消",
};

static ZH_HANT: ConsentCopy = ConsentCopy {
    title: "允許顯示金鑰？",
    subject_label: "",
    warning: "開啟後，第三方擴充功能和角色卡腳本也能讀取全部 API 金鑰。只在需要檢視或複製時開啟，用完後關閉。重新啟動應用程式後生效。",
    http_warning: None,
    confirm: "允許顯示",
    cancel: "取消",
};

static EN: ConsentCopy = ConsentCopy {
    title: "Allow keys exposure?",
    subject_label: "",
    warning: "Once enabled, third-party extensions and character card scripts can also read all API keys. Enable it only when you need to view or copy keys, and turn it off afterward. Takes effect after restart.",
    http_warning: None,
    confirm: "Allow",
    cancel: "Cancel",
};
