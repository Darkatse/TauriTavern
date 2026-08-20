//! `log`：脚本日志收集到结果通道（`SkillScriptResult.logs`）。

use std::cell::RefCell;
use std::rc::Rc;

use rquickjs::{Ctx, Function, Object};

use tt_ports::skill_script::SkillScriptLogLevel;

use crate::api::fs::OverlayFs;

/// 构建 `log` 对象：info / warn / error / debug。
/// 由 `@tauritavern/runtime/v1` 原生模块导出，不再注入全局。
pub(crate) fn build_log_object<'js>(
    ctx: &Ctx<'js>,
    overlay: Rc<RefCell<OverlayFs>>,
) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;

    for (name, level) in [
        ("info", SkillScriptLogLevel::Info),
        ("warn", SkillScriptLogLevel::Warn),
        ("error", SkillScriptLogLevel::Error),
        ("debug", SkillScriptLogLevel::Debug),
    ] {
        let log_overlay = overlay.clone();
        let function = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'_>, message: String| -> Result<(), rquickjs::Error> {
                log_overlay
                    .borrow_mut()
                    .log(level, message)
                    .map_err(|m| rquickjs::Exception::throw_message(&ctx, &m))
            },
        )?;
        object.set(name, function)?;
    }

    Ok(object)
}
