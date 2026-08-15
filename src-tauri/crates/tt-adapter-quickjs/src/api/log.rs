//! `$log`：脚本日志输出到宿主 log（无返回值）。

use rquickjs::{Ctx, Function, Object};

pub(crate) fn register_log_api<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let object = Object::new(ctx.clone())?;
    for (name, level) in [
        ("info", log::Level::Info),
        ("warn", log::Level::Warn),
        ("error", log::Level::Error),
        ("debug", log::Level::Debug),
    ] {
        let function = Function::new(ctx.clone(), move |message: String| {
            log::log!(level, "[skill-script] {message}");
        })?;
        object.set(name, function)?;
    }
    globals.set("$log", object)?;
    Ok(())
}
