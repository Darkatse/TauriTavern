use tauri::WebviewWindow;

/// Applies the iOS WKWebView host policy required by TauriTavern's browser contract.
pub fn configure_main_wkwebview(window: &WebviewWindow) -> tauri::Result<()> {
    let host_window = window.clone();
    window.with_webview(move |webview| unsafe {
        use objc2::runtime::AnyObject;

        let wkwebview_ptr = webview.inner();
        assert!(
            !wkwebview_ptr.is_null(),
            "PlatformWebview.inner() returned a null WKWebView pointer"
        );

        let wkwebview = &*wkwebview_ptr.cast::<AnyObject>();
        super::apple_webview_refresh_rate::enable_native_refresh_rate(wkwebview);
        disable_content_inset_adjustment(wkwebview);
        crate::platform::ios_window_layout::install(
            &*wkwebview_ptr.cast::<objc2_ui_kit::UIView>(),
            host_window,
        );
        super::apple_webview_js_dialogs::install_js_dialog_ui_delegate(wkwebview);
    })
}

unsafe fn disable_content_inset_adjustment(wkwebview: &objc2::runtime::AnyObject) {
    use objc2::rc::Retained;
    use objc2_ui_kit::{
        UIEdgeInsetsZero, UIScrollView, UIScrollViewContentInsetAdjustmentBehavior,
    };

    let scroll_view: Retained<UIScrollView> = objc2::msg_send![wkwebview, scrollView];
    let zero_insets = unsafe { UIEdgeInsetsZero };
    scroll_view
        .setContentInsetAdjustmentBehavior(UIScrollViewContentInsetAdjustmentBehavior::Never);
    scroll_view.setContentInset(zero_insets);
    scroll_view.setScrollIndicatorInsets(zero_insets);
    scroll_view.setAutomaticallyAdjustsScrollIndicatorInsets(false);
}
