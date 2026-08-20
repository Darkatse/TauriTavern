//! `workspace`：操作内存覆盖层的工作区文件 API。
//!
//! 所有读写均针对内存中的 `OverlayFs`，不接触物理文件系统。
//! 写入操作被收集到 `writes` 通道，由应用层在执行完成后落盘。

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rquickjs::{Ctx, Function, Object};

use tt_ports::skill_script::{SkillScriptLogLevel, SkillScriptLog};

/// 内存覆盖文件系统：快照 + 写入收集器 + 日志收集器。
pub(crate) struct OverlayFs {
    /// 初始快照 + 脚本写入的叠加状态。
    files: HashMap<String, String>,
    /// 可见根前缀。
    visible_roots: Vec<String>,
    /// 可写根前缀。
    writable_roots: Vec<String>,
    /// 最终状态写入 map：同路径 insert 覆盖，天然去重为最终 delta。
    pub writes: BTreeMap<String, String>,
    /// 收集的日志。
    pub logs: Vec<SkillScriptLog>,
}

impl OverlayFs {
    pub fn new(
        snapshot: HashMap<String, String>,
        visible_roots: Vec<String>,
        writable_roots: Vec<String>,
    ) -> Self {
        Self {
            files: snapshot,
            visible_roots,
            writable_roots,
            writes: BTreeMap::new(),
            logs: Vec::new(),
        }
    }

    fn is_under_roots(cleaned: &str, roots: &[String]) -> bool {
        roots.iter().any(|root| {
            let root = root.trim();
            !root.is_empty() && {
                let root = root.trim_end_matches(['/', '\\']);
                cleaned == root
                    || cleaned.starts_with(&format!("{root}/"))
                    || cleaned.starts_with(&format!("{root}\\"))
            }
        })
    }

    /// 清洗相对路径：拒绝绝对路径与 `..` 逃逸。
    fn clean_path(raw: &str) -> Result<String, String> {
        if raw.contains('\0') {
            return Err(format!("path must not contain NUL: {raw:?}"));
        }
        if Path::new(raw).is_absolute() {
            return Err(format!("absolute paths are not allowed: {raw}"));
        }
        let cleaned = path_clean::clean(raw)
            .to_string_lossy()
            .replace('\\', "/");
        if cleaned.starts_with("..") {
            return Err(format!("path escapes the workspace: {raw}"));
        }
        Ok(cleaned)
    }

    pub fn read_text(&mut self, raw: &str) -> Result<String, String> {
        let cleaned = Self::clean_path(raw)?;
        if !Self::is_under_roots(&cleaned, &self.visible_roots) {
            return Err(format!("path is outside the visible workspace roots: {raw}"));
        }
        self.files
            .get(&cleaned)
            .cloned()
            .ok_or_else(|| format!("file not found: {raw}"))
    }

    pub fn write_text(&mut self, raw: &str, content: String) -> Result<(), String> {
        let cleaned = Self::clean_path(raw)?;
        if !Self::is_under_roots(&cleaned, &self.writable_roots) {
            return Err(format!("path is outside the writable workspace roots: {raw}"));
        }
        self.files.insert(cleaned.clone(), content.clone());
        // 最终状态 map：同一路径覆盖，天然去重为最终 delta
        self.writes.insert(cleaned, content);
        Ok(())
    }

    pub fn list_files(&self, raw: Option<&str>) -> Result<Vec<String>, String> {
        let prefix = match raw {
            None => String::new(),
            Some(p) => {
                let cleaned = Self::clean_path(p)?;
                if !Self::is_under_roots(&cleaned, &self.visible_roots) {
                    return Err(format!(
                        "path is outside the visible workspace roots: {p}"
                    ));
                }
                cleaned.trim_end_matches(['/', '\\']).to_string()
            }
        };

        let mut entries: Vec<String> = self
            .files
            .keys()
            .filter_map(|path| {
                if prefix.is_empty() {
                    // 列顶层：返回路径的第一段
                    let first_segment = path.split(['/', '\\']).next().unwrap_or(path);
                    Some(first_segment.to_string())
                } else if path.starts_with(&format!("{prefix}/")) || path == prefix.as_str() {
                    // 列指定目录下：返回 prefix 之后的相对路径
                    let rest = &path[prefix.len()..];
                    let rest = rest.trim_start_matches(['/', '\\']);
                    if rest.is_empty() {
                        None
                    } else {
                        Some(rest.to_string())
                    }
                } else {
                    None
                }
            })
            .collect();
        entries.sort();
        entries.dedup();
        Ok(entries)
    }

    pub fn exists(&self, raw: &str) -> bool {
        let cleaned = match Self::clean_path(raw) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if !Self::is_under_roots(&cleaned, &self.visible_roots) {
            return false;
        }
        self.files.contains_key(&cleaned)
    }

    pub fn log(&mut self, level: SkillScriptLogLevel, message: String) {
        self.logs.push(SkillScriptLog { level, message });
    }
}

fn js_error<'js>(ctx: &Ctx<'js>, message: String) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, &message)
}

/// 构建 `workspace` 对象：readText / writeText / listFiles / exists。
/// 由 `@tauritavern/runtime/v1` 原生模块导出，不再注入全局。
pub(crate) fn build_workspace_object<'js>(
    ctx: &Ctx<'js>,
    overlay: std::rc::Rc<RefCell<OverlayFs>>,
) -> rquickjs::Result<Object<'js>> {
    let fs_object = Object::new(ctx.clone())?;

    // readText(path) → string
    let read_overlay = overlay.clone();
    let read_text = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: String| -> Result<String, rquickjs::Error> {
            let mut fs = read_overlay.borrow_mut();
            fs.read_text(&path).map_err(|m| js_error(&ctx, m))
        },
    )?;
    fs_object.set("readText", read_text)?;

    // writeText(path, content) → void
    let write_overlay = overlay.clone();
    let write_text = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: String, content: String| -> Result<(), rquickjs::Error> {
            let mut fs = write_overlay.borrow_mut();
            fs.write_text(&path, content).map_err(|m| js_error(&ctx, m))
        },
    )?;
    fs_object.set("writeText", write_text)?;

    // listFiles(path?) → string[]
    let list_overlay = overlay.clone();
    let list_files = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: Option<String>| -> Result<Vec<String>, rquickjs::Error> {
            let fs = list_overlay.borrow();
            fs.list_files(path.as_deref()).map_err(|m| js_error(&ctx, m))
        },
    )?;
    fs_object.set("listFiles", list_files)?;

    // exists(path) → boolean
    let exists_overlay = overlay.clone();
    let exists = Function::new(
        ctx.clone(),
        move |_ctx: Ctx<'_>, path: String| -> Result<bool, rquickjs::Error> {
            let fs = exists_overlay.borrow();
            Ok(fs.exists(&path))
        },
    )?;
    fs_object.set("exists", exists)?;

    Ok(fs_object)
}
