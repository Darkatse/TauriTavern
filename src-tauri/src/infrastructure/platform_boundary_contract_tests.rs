use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn platform_boundary_stays_out_of_inner_layers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");

    assert_no_layer_refs(&root.join("domain"), "platform");
    assert_no_layer_refs(&root.join("application"), "platform");
}

#[test]
fn platform_adapter_does_not_call_back_into_app_layers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/platform");

    for forbidden in ["app", "application", "infrastructure", "presentation"] {
        assert_no_layer_refs(&root, forbidden);
    }
}

fn assert_no_layer_refs(root: &Path, forbidden: &str) {
    for path in rust_files(root) {
        let source = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("read {}: {}", path.display(), error);
        });
        let code = source
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains(&format!("crate::{forbidden}"))
                && !code.contains(&format!("{forbidden}::")),
            "{} must not reference `{forbidden}`",
            path.display()
        );
    }
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files);
    files
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path.to_path_buf());
        }
        return;
    }

    for entry in fs::read_dir(path).unwrap_or_else(|error| {
        panic!("read directory {}: {}", path.display(), error);
    }) {
        let entry = entry.expect("read directory entry");
        collect_rust_files(&entry.path(), files);
    }
}
