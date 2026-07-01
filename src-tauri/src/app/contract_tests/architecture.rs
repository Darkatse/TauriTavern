use std::fs;
use std::path::Path;

#[test]
fn infrastructure_does_not_depend_on_application() {
    let infrastructure = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("infrastructure");
    let mut offenders = Vec::new();

    scan_rs_files(&infrastructure, &mut |path, content| {
        for (index, line) in content.lines().enumerate() {
            if line.contains("crate::application::") {
                offenders.push(format!("{}:{}", path.display(), index + 1));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "infrastructure must depend on tt-contracts/tt-ports/tt-domain, not application:\n{}",
        offenders.join("\n")
    );
}

fn scan_rs_files(root: &Path, visit: &mut dyn FnMut(&Path, &str)) {
    for entry in fs::read_dir(root).expect("read infrastructure dir") {
        let path = entry.expect("read infrastructure entry").path();
        if path.is_dir() {
            scan_rs_files(&path, visit);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let content = fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!("read {}: {error}", path.display());
            });
            visit(&path, &content);
        }
    }
}
