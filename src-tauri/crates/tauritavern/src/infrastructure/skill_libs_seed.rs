//! 预置公共 skill 脚本库（`skill.run_script` 裸模块白名单）的种子释放。
//!
//! 库以单文件 ESM 形式编译期内嵌进二进制（`include_str!`），首次启动时释放到
//! `{data_root}/skill-libs/`。desktop 与 Android 行为一致——不依赖 Tauri 资源
//! 目录机制，绕开 Android 对 `BaseDirectory::Resource` 的特殊处理。

use std::path::Path;

use tokio::fs;

use tt_domain::errors::DomainError;

/// 内容指纹清单文件名，随库一起释放到 skill-libs 目录。
/// 下次启动时比对指纹，不一致则覆盖，从而库升级时同步到已安装用户。
const VERSION_MANIFEST_NAME: &str = ".skill-libs-version";

/// 单文件自包含 ESM 库，随二进制内嵌。
const LIBS: &[(&str, &str)] = &[
    ("dayjs.js", include_str!("../../resources/skill-libs/dayjs.js")),
    (
        "es-toolkit.js",
        include_str!("../../resources/skill-libs/es-toolkit.js"),
    ),
    (
        "fast-xml-parser.js",
        include_str!("../../resources/skill-libs/fast-xml-parser.js"),
    ),
    ("marked.js", include_str!("../../resources/skill-libs/marked.js")),
    (
        "papaparse.js",
        include_str!("../../resources/skill-libs/papaparse.js"),
    ),
    ("slugify.js", include_str!("../../resources/skill-libs/slugify.js")),
];

/// 把内嵌库释放到 `libs_dir`：首次创建目录，之后仅在指纹清单变化时覆盖。
pub(crate) async fn seed_bundled_skill_libs(libs_dir: &Path) -> Result<(), DomainError> {
    let manifest = build_version_manifest();

    if is_up_to_date(libs_dir, &manifest).await {
        tracing::info!("skill-libs 已是最新，无需释放");
        return Ok(());
    }

    fs::create_dir_all(libs_dir).await.map_err(|error| {
        tracing::error!("Failed to create skill-libs dir {:?}: {}", libs_dir, error);
        DomainError::InternalError(format!("Failed to create skill-libs dir: {}", error))
    })?;

    for (name, contents) in LIBS {
        let target = libs_dir.join(name);
        fs::write(&target, contents).await.map_err(|error| {
            tracing::error!("Failed to write skill lib {:?}: {}", target, error);
            DomainError::InternalError(format!("Failed to write skill lib {}: {}", name, error))
        })?;
    }

    fs::write(libs_dir.join(VERSION_MANIFEST_NAME), &manifest)
        .await
        .map_err(|error| {
            tracing::error!("Failed to write skill-libs version manifest: {}", error);
            DomainError::InternalError(format!(
                "Failed to write skill-libs version manifest: {}",
                error
            ))
        })?;

    tracing::info!("skill-libs 已释放/更新到 {:?}", libs_dir);
    Ok(())
}

/// 用所有内嵌库内容的指纹构建清单，任一库变化都会改变清单。
/// 指纹 = 长度 + FNV-1a 滚动哈希，不引入额外哈希 crate（保持对原项目低依赖侵入），
/// 对版本检测足够可靠。
fn build_version_manifest() -> String {
    let mut combined = String::new();
    for (name, contents) in LIBS {
        combined.push_str(name);
        combined.push(':');
        combined.push_str(&fingerprint(contents));
        combined.push('\n');
    }
    combined
}

fn fingerprint(contents: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let bytes = contents.as_bytes();
    let mut hash = FNV_OFFSET;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{}:{}", bytes.len(), hash)
}

async fn is_up_to_date(libs_dir: &Path, manifest: &str) -> bool {
    match fs::read_to_string(libs_dir.join(VERSION_MANIFEST_NAME)).await {
        Ok(existing) => existing == manifest,
        Err(_) => false,
    }
}
