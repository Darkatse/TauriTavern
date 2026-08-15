//! `$fs`：受 SandboxIoPolicy 门控的同步文件 API（执行线程已在 spawn_blocking 中）。

use rquickjs::{Ctx, Function, Object};

use crate::sandbox::SandboxIoPolicy;

fn js_error<'js>(ctx: &Ctx<'js>, message: String) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, &message)
}

pub(crate) fn register_fs_api<'js>(
    ctx: &Ctx<'js>,
    policy: SandboxIoPolicy,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let fs_object = Object::new(ctx.clone())?;

    let read_policy = policy.clone();
    let read_text = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: String| -> Result<String, rquickjs::Error> {
            let target = read_policy
                .check_read(&path)
                .map_err(|message| js_error(&ctx, message))?;
            std::fs::read_to_string(&target)
                .map_err(|error| js_error(&ctx, format!("failed to read `{path}`: {error}")))
        },
    )?;

    let write_policy = policy.clone();
    let write_text = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: String, content: String| -> Result<(), rquickjs::Error> {
            let target = write_policy
                .check_write(&path)
                .map_err(|message| js_error(&ctx, message))?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    js_error(&ctx, format!("failed to create directory for `{path}`: {error}"))
                })?;
            }
            std::fs::write(&target, content)
                .map_err(|error| js_error(&ctx, format!("failed to write `{path}`: {error}")))
        },
    )?;

    let list_policy = policy.clone();
    let list_files = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'_>, path: Option<String>| -> Result<Vec<String>, rquickjs::Error> {
            // 无参：列出 work_dir 顶层条目名（仅名字，无内容）；
            // 有参：读取该目录下条目的 work_dir 相对路径，读权限同 check_read。
            let mut entries = Vec::new();
            let base = match path.as_deref() {
                None => list_policy.work_dir.clone(),
                Some(path) => list_policy
                    .check_read(path)
                    .map_err(|message| js_error(&ctx, message))?,
            };
            let directory = std::fs::read_dir(&base).map_err(|error| {
                js_error(&ctx, format!("failed to list `{}`: {error}", base.display()))
            })?;
            for entry in directory {
                let entry = entry
                    .map_err(|error| js_error(&ctx, format!("failed to read entry: {error}")))?;
                let name = entry.file_name().to_string_lossy().to_string();
                entries.push(match path.as_deref() {
                    None => name,
                    Some(prefix) => {
                        let prefix = prefix.trim_end_matches(['/', '\\']);
                        format!("{prefix}/{name}")
                    }
                });
            }
            entries.sort();
            Ok(entries)
        },
    )?;

    let exists_policy = policy.clone();
    let exists = Function::new(
        ctx.clone(),
        move |_ctx: Ctx<'_>, path: String| -> Result<bool, rquickjs::Error> {
            match exists_policy.check_read(&path) {
                Ok(target) => Ok(target.is_file() || target.is_dir()),
                Err(_) => Ok(false),
            }
        },
    )?;

    fs_object.set("readText", read_text)?;
    fs_object.set("writeText", write_text)?;
    fs_object.set("listFiles", list_files)?;
    fs_object.set("exists", exists)?;
    globals.set("$fs", fs_object)?;
    Ok(())
}
