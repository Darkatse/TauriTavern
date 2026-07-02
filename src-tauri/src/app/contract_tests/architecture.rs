use std::fs;
use std::path::Path;

#[test]
fn outer_layers_do_not_depend_on_application() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [
        manifest_dir.join("src").join("infrastructure"),
        manifest_dir.join("src").join("platform"),
    ];
    let forbidden = ["crate::application::", "tt_application::"];
    let mut offenders = Vec::new();

    for root in roots {
        scan_rs_files(&root, &mut |path, content| {
            for (index, line) in content.lines().enumerate() {
                for pattern in forbidden {
                    if line.contains(pattern) {
                        offenders.push(format!("{}:{}: {}", path.display(), index + 1, pattern));
                    }
                }
            }
        });
    }

    assert!(
        offenders.is_empty(),
        "outer layers must depend on tt-contracts/tt-ports/tt-domain, not application:\n{}",
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
