#[test]
fn js_dialog_delegate_is_centralized_in_apple_module() {
    use std::fs;
    use std::path::PathBuf;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let apple = fs::read_to_string(root.join("src/infrastructure/apple_webview_js_dialogs.rs"))
        .expect("read apple_webview_js_dialogs.rs");
    assert!(
        apple.contains("define_class!("),
        "apple_webview_js_dialogs.rs must own the WKUIDelegate proxy class"
    );
    assert!(
        apple.contains("objc_setAssociatedObject"),
        "apple_webview_js_dialogs.rs must retain the delegate via associated object"
    );

    let ios = fs::read_to_string(root.join("src/infrastructure/ios_webview.rs"))
        .expect("read ios_webview.rs");
    assert!(
        !ios.contains("define_class!("),
        "ios_webview.rs should not define a WKUIDelegate proxy class"
    );
    assert!(
        ios.contains("apple_webview_js_dialogs::install_js_dialog_ui_delegate"),
        "ios_webview.rs must install the shared WKUIDelegate proxy"
    );

    let macos = fs::read_to_string(root.join("src/infrastructure/macos_webview.rs"))
        .expect("read macos_webview.rs");
    assert!(
        !macos.contains("define_class!("),
        "macos_webview.rs should not define a WKUIDelegate proxy class"
    );
    assert!(
        macos.contains("apple_webview_js_dialogs::install_js_dialog_ui_delegate"),
        "macos_webview.rs must install the shared WKUIDelegate proxy"
    );
}
