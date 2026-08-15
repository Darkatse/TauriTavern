use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.ends_with("apple-darwin") {
        return;
    }

    // ORT resolves this through generic CC, which may point at the Android NDK
    // in a multi-target Tauri shell. xcrun is the authoritative macOS toolchain.
    let Ok(output) = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }

    let runtime_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
        .join("lib")
        .join("darwin");
    if runtime_dir.is_dir() {
        println!("cargo:rustc-link-search=native={}", runtime_dir.display());
    }
}
