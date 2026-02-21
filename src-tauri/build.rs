use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../default/content");
    println!("cargo:rerun-if-changed=../src/scripts/templates");
    println!("cargo:rerun-if-changed=../src/scripts/extensions");
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
    let extension_root = PathBuf::from("../src/scripts/extensions");
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

    let extension_template_resources = collect_extension_top_level_html(&extension_root)?;
    embedded_resources.extend(extension_template_resources);

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

fn collect_extension_top_level_html(root: &Path) -> Result<Vec<ResourceEntry>, Box<dyn Error>> {
    let mut resources = Vec::new();

    for extension_entry in fs::read_dir(root)? {
        let extension_entry = extension_entry?;
        let extension_type = extension_entry.file_type()?;
        if !extension_type.is_dir() {
            continue;
        }

        let extension_name = extension_entry.file_name().to_string_lossy().to_string();
        let extension_path = extension_entry.path();

        for file_entry in fs::read_dir(&extension_path)? {
            let file_entry = file_entry?;
            let file_type = file_entry.file_type()?;
            if !file_type.is_file() {
                continue;
            }

            let file_path = file_entry.path();
            let is_html = file_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("html"))
                .unwrap_or(false);

            if !is_html {
                continue;
            }

            let file_name = match file_path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            resources.push(ResourceEntry {
                virtual_path: format!("frontend-extensions/{}/{}", extension_name, file_name),
                source_path: file_path,
            });
        }
    }

    Ok(resources)
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
