//! 单次脚本执行的沙箱 IO 策略：
//! - `$fs` 路径解析相对 `work_dir`，读/写分别受 visible/writable roots 门控，拒绝逃逸；
//! - 模块加载白名单 = 入口脚本所在 skill 的 `scripts/` 目录 + 公共 libs 目录。

use std::path::{Path, PathBuf};

/// 一次执行的不可变 IO 策略。
#[derive(Debug, Clone)]
pub struct SandboxIoPolicy {
    pub work_dir: PathBuf,
    pub visible_roots: Vec<String>,
    pub writable_roots: Vec<String>,
    pub scripts_dir: PathBuf,
    pub libs_dir: PathBuf,
}

impl SandboxIoPolicy {
    pub fn new(
        work_dir: PathBuf,
        visible_roots: Vec<String>,
        writable_roots: Vec<String>,
        scripts_dir: PathBuf,
        libs_dir: PathBuf,
    ) -> Self {
        Self {
            work_dir,
            visible_roots,
            writable_roots,
            scripts_dir,
            libs_dir,
        }
    }

    /// 将脚本提供的相对路径解析到 work_dir 内；拒绝绝对路径与 `..` 逃逸。
    pub fn resolve_work_path(&self, raw: &str) -> Result<PathBuf, String> {
        if raw.contains('\0') {
            return Err(format!("path must not contain NUL: {raw:?}"));
        }
        let relative = Path::new(raw);
        if relative.is_absolute() {
            return Err(format!("absolute paths are not allowed: {raw}"));
        }
        let cleaned = path_clean::clean(relative);
        if cleaned.to_string_lossy().starts_with("..") {
            return Err(format!("path escapes the workspace: {raw}"));
        }
        Ok(self.work_dir.join(cleaned))
    }

    fn under_roots(cleaned: &Path, roots: &[String]) -> bool {
        roots.iter().any(|root| {
            let root = root.trim();
            !root.is_empty() && cleaned.starts_with(Path::new(root))
        })
    }

    /// `$fs` 读门控：路径（清洗后）必须落在某个 visible root 内。
    pub fn check_read(&self, raw: &str) -> Result<PathBuf, String> {
        let cleaned = path_clean::clean(Path::new(raw));
        if !Self::under_roots(&cleaned, &self.visible_roots) {
            return Err(format!("path is outside the visible workspace roots: {raw}"));
        }
        self.resolve_work_path(raw)
    }

    /// `$fs` 写门控：路径（清洗后）必须落在某个 writable root 内。
    pub fn check_write(&self, raw: &str) -> Result<PathBuf, String> {
        let cleaned = path_clean::clean(Path::new(raw));
        if !Self::under_roots(&cleaned, &self.writable_roots) {
            return Err(format!("path is outside the writable workspace roots: {raw}"));
        }
        self.resolve_work_path(raw)
    }

    /// 模块解析门控：
    /// - `./`、`../` 相对导入：清洗后必须仍位于当前 skill 的 `scripts/` 目录内；
    /// - 裸模块名：解析到公共 libs 目录（`{name}.js` 或 `{name}/index.js`）；
    /// - 绝对路径与其他形式一律拒绝。
    pub fn resolve_module(&self, base: &str, specifier: &str) -> Result<PathBuf, String> {
        if specifier.contains('\0') {
            return Err("module specifier must not contain NUL".to_string());
        }
        if specifier.starts_with("./") || specifier.starts_with("../") {
            let base_dir = Path::new(base)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.scripts_dir.clone());
            let direct = path_clean::clean(base_dir.join(specifier));
            for candidate in [
                direct.clone(),
                path_clean::clean(format!("{}.js", direct.display())),
                path_clean::clean(direct.join("index.js")),
            ] {
                if candidate.starts_with(&self.scripts_dir) && candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!(
                "module `{specifier}` was not found inside the skill scripts directory"
            ))
        } else {
            if Path::new(specifier).is_absolute() {
                return Err(format!(
                    "module `{specifier}` must not be an absolute path"
                ));
            }
            for candidate in [
                self.libs_dir.join(format!("{specifier}.js")),
                self.libs_dir.join(specifier).join("index.js"),
            ] {
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!(
                "module `{specifier}` was not found in the public skill libraries"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SandboxIoPolicy;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn policy_with_dirs(scripts_dir: PathBuf, libs_dir: PathBuf) -> SandboxIoPolicy {
        SandboxIoPolicy::new(
            PathBuf::from("/tmp/work"),
            vec!["output".to_string()],
            vec!["output".to_string()],
            scripts_dir,
            libs_dir,
        )
    }

    #[test]
    fn read_is_gated_by_visible_roots() {
        let policy = policy_with_dirs(PathBuf::from("/tmp/scripts"), PathBuf::from("/tmp/libs"));

        assert!(policy.check_read("output/a.md").is_ok());
        assert!(policy.check_read("input/secret.json").is_err());
    }

    #[test]
    fn write_is_gated_by_writable_roots() {
        let policy = SandboxIoPolicy::new(
            PathBuf::from("/tmp/work"),
            vec!["output".to_string()],
            vec![], // nothing writable
            PathBuf::from("/tmp/scripts"),
            PathBuf::from("/tmp/libs"),
        );

        assert!(policy.check_write("output/a.md").is_err());
        assert!(policy.check_read("output/a.md").is_ok());
    }

    #[test]
    fn escapes_and_absolute_paths_are_rejected() {
        let policy = policy_with_dirs(PathBuf::from("/tmp/scripts"), PathBuf::from("/tmp/libs"));

        assert!(policy.check_read("../outside.md").is_err());
        assert!(policy.check_read("output/../../outside.md").is_err());
        assert!(policy.check_read("/etc/passwd").is_err());
    }

    #[test]
    fn relative_modules_must_stay_in_scripts_dir() {
        // 使用真实临时目录：相对模块解析要求目标文件实际存在（is_file 门控），
        // 虚构的 /tmp 路径在 Windows 上不存在会导致误判。
        let temp = TempDir::new().unwrap();
        let scripts_dir = temp.path().join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();
        std::fs::write(scripts_dir.join("helper.js"), "export const h = 1;").unwrap();
        let policy = policy_with_dirs(scripts_dir.clone(), temp.path().join("libs"));

        let base = scripts_dir.join("main.js").to_string_lossy().to_string();
        assert!(policy.resolve_module(&base, "./helper.js").is_ok());
        // ../lib.js 清洗后落在 scripts 目录外 -> 拒绝（即使文件不存在）
        assert!(policy.resolve_module(&base, "../lib.js").is_err());
        assert!(policy.resolve_module(&base, "/abs/path.js").is_err());
    }

    #[test]
    fn bare_modules_resolve_to_public_libs() {
        let temp = TempDir::new().unwrap();
        let libs_dir = temp.path().join("skill-libs");
        std::fs::create_dir_all(libs_dir.join("markdown")).unwrap();
        std::fs::write(libs_dir.join("markdown").join("index.js"), "export const x = 1;").unwrap();
        std::fs::write(libs_dir.join("utils.js"), "export const y = 2;").unwrap();
        let policy = policy_with_dirs(PathBuf::from("/tmp/scripts"), libs_dir.clone());

        let base = PathBuf::from("/tmp/scripts/main.js").to_string_lossy().to_string();
        let direct = policy.resolve_module(&base, "utils").expect("utils resolves");
        assert!(direct.ends_with("utils.js"));
        let index = policy
            .resolve_module(&base, "markdown")
            .expect("markdown resolves");
        assert!(index.to_string_lossy().contains("markdown"));
        assert!(policy.resolve_module(&base, "missing-lib").is_err());
    }
}
