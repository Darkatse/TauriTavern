use tauri::WebviewWindow;

pub fn disable_wkwebview_content_inset_adjustment(window: &WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| unsafe {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2_ui_kit::{
            UIEdgeInsetsZero, UIScrollView, UIScrollViewContentInsetAdjustmentBehavior,
        };

        let wkwebview_ptr = webview.inner();
        assert!(
            !wkwebview_ptr.is_null(),
            "PlatformWebview.inner() returned a null WKWebView pointer"
        );

        let wkwebview = &*wkwebview_ptr.cast::<AnyObject>();
        let scroll_view: Retained<UIScrollView> = objc2::msg_send![wkwebview, scrollView];

        scroll_view
            .setContentInsetAdjustmentBehavior(UIScrollViewContentInsetAdjustmentBehavior::Never);
        scroll_view.setContentInset(UIEdgeInsetsZero);
        scroll_view.setScrollIndicatorInsets(UIEdgeInsetsZero);
        scroll_view.setAutomaticallyAdjustsScrollIndicatorInsets(false);
    })
}
