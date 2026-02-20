use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../default/content");
    println!("cargo:rerun-if-changed=../src/scripts/templates");
    println!("cargo:rerun-if-changed=../src/scripts/extensions/regex");
    println!("cargo:rerun-if-changed=../src/scripts/extensions/code-render");
    println!("cargo:rerun-if-changed=../src/scripts/extensions/data-migration");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
    println!("cargo:rerun-if-env-changed=GITHUB_REF_NAME");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");

    emit_git_build_metadata();

    if let Err(error) = generate_resource_artifacts() {
        panic!("Failed to generate resource artifacts: {}", error);
    }

    tauri_build::build()
}

fn emit_git_build_metadata() {
    let git_branch = normalize_git_branch(
        std::env::var("GITHUB_REF_NAME")
            .ok()
            .or_else(|| run_git_command(&["rev-parse", "--abbrev-ref", "HEAD"])),
    );

    let git_revision = normalize_git_value(
        std::env::var("GITHUB_SHA")
            .ok()
            .map(|sha| shorten_revision(&sha))
            .or_else(|| run_git_command(&["rev-parse", "--short=12", "HEAD"])),
    );

    println!(
        "cargo:rustc-env=TAURITAVERN_GIT_BRANCH={}",
        git_branch.unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=TAURITAVERN_GIT_REVISION={}",
        git_revision.unwrap_or_default()
    );
}

fn run_git_command(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

fn normalize_git_value(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let normalized = value.trim();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_string())
        }
    })
}

fn normalize_git_branch(value: Option<String>) -> Option<String> {
    let branch = normalize_git_value(value)?;
    if branch.eq_ignore_ascii_case("head") {
        None
    } else {
        Some(branch)
    }
}

fn shorten_revision(value: &str) -> String {
    value.trim().chars().take(12).collect()
}

#[derive(Debug)]
struct ResourceEntry {
    virtual_path: String,
    source_path: PathBuf,
}

fn generate_resource_artifacts() -> Result<(), Box<dyn Error>> {
    let content_root = PathBuf::from("../default/content");
    let template_root = PathBuf::from("../src/scripts/templates");
    let regex_template_root = PathBuf::from("../src/scripts/extensions/regex");
    let code_render_template_root = PathBuf::from("../src/scripts/extensions/code-render");
    let data_migration_template_root = PathBuf::from("../src/scripts/extensions/data-migration");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);

    let mut content_files = collect_relative_files(&content_root, &content_root)?;
    content_files.sort();

    fs::write(
        out_dir.join("default_content_manifest.json"),
        serde_json::to_string(&content_files)?,
    )?;

    let mut embedded_resources = Vec::new();
    embedded_resources.extend(
        content_files
            .iter()
            .map(|relative| ResourceEntry {
                virtual_path: format!("default/content/{}", relative),
                source_path: content_root.join(relative),
            })
            .collect::<Vec<_>>(),
    );

    let template_files = collect_relative_files(&template_root, &template_root)?;
    embedded_resources.extend(
        template_files
            .iter()
            .map(|relative| ResourceEntry {
                virtual_path: format!("frontend-templates/{}", relative),
                source_path: template_root.join(relative),
            })
            .collect::<Vec<_>>(),
    );

    let regex_template_files = collect_relative_files(&regex_template_root, &regex_template_root)?
        .into_iter()
        .filter(|relative| relative.ends_with(".html"))
        .collect::<Vec<_>>();
    embedded_resources.extend(
        regex_template_files
            .iter()
            .map(|relative| ResourceEntry {
                virtual_path: format!("frontend-extensions/regex/{}", relative),
                source_path: regex_template_root.join(relative),
            })
            .collect::<Vec<_>>(),
    );

    let code_render_template_files =
        collect_relative_files(&code_render_template_root, &code_render_template_root)?
            .into_iter()
            .filter(|relative| relative.ends_with(".html"))
            .collect::<Vec<_>>();
    embedded_resources.extend(
        code_render_template_files
            .iter()
            .map(|relative| ResourceEntry {
                virtual_path: format!("frontend-extensions/code-render/{}", relative),
                source_path: code_render_template_root.join(relative),
            })
            .collect::<Vec<_>>(),
    );

    let data_migration_template_files =
        collect_relative_files(&data_migration_template_root, &data_migration_template_root)?
            .into_iter()
            .filter(|relative| relative.ends_with(".html"))
            .collect::<Vec<_>>();
    embedded_resources.extend(
        data_migration_template_files
            .iter()
            .map(|relative| ResourceEntry {
                virtual_path: format!("frontend-extensions/data-migration/{}", relative),
                source_path: data_migration_template_root.join(relative),
            })
            .collect::<Vec<_>>(),
    );

    embedded_resources.sort_by(|a, b| a.virtual_path.cmp(&b.virtual_path));

    fs::write(
        out_dir.join("embedded_resources.rs"),
        build_embedded_resources_source(&embedded_resources)?,
    )?;

    Ok(())
}

fn collect_relative_files(root: &Path, current: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            files.extend(collect_relative_files(root, &path)?);
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(relative);
        }
    }

    Ok(files)
}

fn build_embedded_resources_source(resources: &[ResourceEntry]) -> Result<String, Box<dyn Error>> {
    let mut source =
        String::from("pub fn get_embedded_resource(path: &str) -> Option<&'static [u8]> {\n");
    source.push_str("    match path {\n");

    for resource in resources {
        let canonical = resource.source_path.canonicalize()?;
        let include_path = canonical.to_string_lossy().replace('\\', "/");
        source.push_str(&format!(
            "        {:?} => Some(include_bytes!(r#\"{}\"#)),\n",
            resource.virtual_path, include_path
        ));
    }

    source.push_str("        _ => None,\n");
    source.push_str("    }\n");
    source.push_str("}\n");

    Ok(source)
}
