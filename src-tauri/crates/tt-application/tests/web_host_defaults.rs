#[path = "../examples/support/web_host_defaults.rs"]
mod defaults;

#[test]
fn web_host_starts_with_the_native_defaults() {
    let published: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../src/tauri/web-host/default-state.json"
    ))
    .unwrap();
    let native = defaults::native_defaults().unwrap();
    assert_eq!(
        published,
        native,
        "Native defaults changed; regenerate the Web Host copy:\n  \
         cargo run --quiet --manifest-path src-tauri/Cargo.toml -p tt-application \
         --example web_host_defaults > src/tauri/web-host/default-state.json",
    );
}
