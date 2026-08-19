//! `$log`：脚本日志收集到结果通道（`SkillScriptResult.logs`）。

use std::cell::RefCell;
use std::rc::Rc;

use rquickjs::{Ctx, Function, Object};

use tt_ports::skill_script::SkillScriptLogLevel;

use crate::api::fs::OverlayFs;

pub(crate) fn register_log_api<'js>(
    ctx: &Ctx<'js>,
    overlay: Rc<RefCell<OverlayFs>>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
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
            move |message: String| {
                log_overlay.borrow_mut().log(level, message);
            },
        )?;
        object.set(name, function)?;
    }

    globals.set("$log", object)?;
    Ok(())
}
