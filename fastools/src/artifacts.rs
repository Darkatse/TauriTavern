use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Copy)]
pub enum BuildArtifactsKind {
    DesktopRelease,
    DesktopDebug,
    AndroidSplitApk,
    IosRelease,
}

pub struct CollectedArtifact {
    destination: PathBuf,
}

impl CollectedArtifact {
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

struct ArtifactCopy {
    source: PathBuf,
    destination_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TauriConfig {
    product_name: String,
    version: String,
}

pub fn collect(project_root: &Path, kind: BuildArtifactsKind) -> Result<Vec<CollectedArtifact>> {
    let config = load_tauri_config(project_root)?;
    let release_dir = project_root.join("release");
    fs::create_dir_all(&release_dir).with_context(|| {
        format!(
            "Failed to create release directory: {}",
            release_dir.display()
        )
    })?;

    let copies = match kind {
        BuildArtifactsKind::DesktopRelease => desktop_artifact_copies(project_root, false)?,
        BuildArtifactsKind::DesktopDebug => desktop_artifact_copies(project_root, true)?,
        BuildArtifactsKind::AndroidSplitApk => android_artifact_copies(project_root, &config)?,
        BuildArtifactsKind::IosRelease => ios_artifact_copies(project_root, &config)?,
    };

    let mut collected = Vec::with_capacity(copies.len());
    for copy in copies {
        let destination = release_dir.join(&copy.destination_name);
        fs::copy(&copy.source, &destination).with_context(|| {
            format!(
                "Failed to copy artifact from {} to {}",
                copy.source.display(),
                destination.display()
            )
        })?;
        collected.push(CollectedArtifact { destination });
    }

    Ok(collected)
}

fn load_tauri_config(project_root: &Path) -> Result<TauriConfig> {
    let config_path = project_root.join("src-tauri/tauri.conf.json");
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", config_path.display()))
}

fn desktop_artifact_copies(project_root: &Path, debug: bool) -> Result<Vec<ArtifactCopy>> {
    let bundle_dir = if debug {
        project_root.join("src-tauri/target/debug/bundle")
    } else {
        project_root.join("src-tauri/target/release/bundle")
    };
    ensure_dir(&bundle_dir)?;

    let mut copies = WalkDir::new(&bundle_dir)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| entry.into_path())
        .filter(|path| is_desktop_distributable(path))
        .map(|source| {
            let file_name = source
                .file_name()
                .and_then(|value| value.to_str())
                .with_context(|| format!("Invalid desktop artifact path: {}", source.display()))?;
            let destination_name = if debug {
                append_debug_marker(file_name)?
            } else {
                file_name.to_owned()
            };
            Ok(ArtifactCopy {
                source,
                destination_name,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    copies.sort_by(|left, right| left.destination_name.cmp(&right.destination_name));

    if copies.is_empty() {
        bail!(
            "No desktop distributable artifacts were found in {}",
            bundle_dir.display()
        );
    }

    Ok(copies)
}

fn android_artifact_copies(project_root: &Path, config: &TauriConfig) -> Result<Vec<ArtifactCopy>> {
    let apk_root = project_root.join("src-tauri/gen/android/app/build/outputs/apk");
    ensure_dir(&apk_root)?;

    let mut abi_dirs = fs::read_dir(&apk_root)
        .with_context(|| format!("Failed to read {}", apk_root.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("Failed to read {}", apk_root.display()))?;
    abi_dirs.sort_by_key(|entry| entry.file_name());

    let mut copies = Vec::new();
    for entry in abi_dirs {
        if !entry
            .file_type()
            .with_context(|| format!("Failed to inspect {}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }

        let abi = entry.file_name();
        let abi = abi.to_str().with_context(|| {
            format!("Invalid Android ABI directory: {}", entry.path().display())
        })?;
        let release_dir = entry.path().join("release");
        if !release_dir.is_dir() {
            continue;
        }

        let mut apks = fs::read_dir(&release_dir)
            .with_context(|| format!("Failed to read {}", release_dir.display()))?
            .filter_map(std::result::Result::ok)
            .map(|item| item.path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("apk"))
            .collect::<Vec<_>>();

        apks.sort();

        let source = match apks.as_slice() {
            [source] => source.clone(),
            [] => continue,
            _ => {
                bail!(
                    "Expected exactly one APK in {}, found {}",
                    release_dir.display(),
                    apks.len()
                )
            }
        };

        copies.push(ArtifactCopy {
            source,
            destination_name: format!(
                "{}-{}-{}-release.apk",
                config.product_name,
                config.version,
                normalize_android_abi(abi)
            ),
        });
    }

    if copies.is_empty() {
        bail!(
            "No Android split APK artifacts were found in {}",
            apk_root.display()
        );
    }

    Ok(copies)
}

fn ios_artifact_copies(project_root: &Path, config: &TauriConfig) -> Result<Vec<ArtifactCopy>> {
    let source = project_root
        .join("src-tauri/gen/apple/build/arm64")
        .join(format!("{}.ipa", config.product_name));
    ensure_file(&source)?;

    Ok(vec![ArtifactCopy {
        source,
        destination_name: format!("{}-{}.ipa", config.product_name, config.version),
    }])
}

fn append_debug_marker(file_name: &str) -> Result<String> {
    let (stem, extension) = file_name
        .rsplit_once('.')
        .with_context(|| format!("Desktop artifact is missing an extension: {file_name}"))?;
    Ok(format!("{stem}-DEBUG.{extension}"))
}

fn is_desktop_distributable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    if path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.ends_with(".AppImage"))
    {
        return true;
    }

    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("dmg" | "deb" | "rpm" | "msi" | "exe")
    )
}

fn normalize_android_abi(abi: &str) -> &str {
    match abi {
        "arm" => "armeabi-v7a",
        "arm64" => "arm64",
        "x86" => "x86",
        "x86_64" => "x86_64",
        other => other,
    }
}

fn ensure_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        Ok(())
    } else {
        bail!("Directory does not exist: {}", path.display())
    }
}

fn ensure_file(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        bail!("File does not exist: {}", path.display())
    }
}
