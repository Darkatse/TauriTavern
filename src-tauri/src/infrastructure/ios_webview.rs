use tauri::WebviewWindow;

/// Applies the iOS WKWebView host policy required by TauriTavern's browser contract.
pub fn configure_main_wkwebview(window: &WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| unsafe {
        use objc2::runtime::AnyObject;

        let wkwebview_ptr = webview.inner();
        assert!(
            !wkwebview_ptr.is_null(),
            "PlatformWebview.inner() returned a null WKWebView pointer"
        );

        let wkwebview = &*wkwebview_ptr.cast::<AnyObject>();
        disable_content_inset_adjustment(wkwebview);
        enable_element_fullscreen(wkwebview);
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

unsafe fn enable_element_fullscreen(wkwebview: &objc2::runtime::AnyObject) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;

    let configuration: Retained<AnyObject> = objc2::msg_send![wkwebview, configuration];
    let preferences: Retained<AnyObject> = objc2::msg_send![&*configuration, preferences];
    let _: () = objc2::msg_send![&*preferences, setElementFullscreenEnabled: true];
}
