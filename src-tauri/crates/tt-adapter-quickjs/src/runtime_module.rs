//! `@tauritavern/runtime`：skill 脚本 Runtime API 原生模块。
//!
//! 脚本经 `import { context, workspace, log } from '@tauritavern/runtime'`
//! 访问宿主能力，沙箱不再注入任何全局对象。每次执行的状态（overlay、
//! Application 上下文）经 `Ctx::store_userdata` 传入，由 `ModuleDef::evaluate`
//! 在模块求值时构建并导出。

use std::cell::RefCell;
use std::rc::Rc;

use rquickjs::module::{Declarations, Exports, ModuleDef};
use rquickjs::{Ctx, JsLifetime, Result};
use serde_json::Value;

use crate::api::fs::OverlayFs;

pub(crate) const RUNTIME_MODULE_NAME: &str = "@tauritavern/runtime";

/// 一次执行的 Runtime 状态，经 ctx userdata 传给原生模块。
pub(crate) struct RuntimeState {
    pub(crate) overlay: Rc<RefCell<OverlayFs>>,
    pub(crate) context: Value,
}

// 纯 Rust 数据（不含任何 rquickjs 'js 引用），Changed<'to> 即自身，
// 与 rquickjs 对 String/Vec 等类型的生成 impl 语义一致。
unsafe impl<'js> JsLifetime<'js> for RuntimeState {
    type Changed<'to> = RuntimeState;
}

pub(crate) struct RuntimeModule;

impl ModuleDef for RuntimeModule {
    fn declare(decl: &Declarations<'_>) -> Result<()> {
        decl.declare("workspace")?;
        decl.declare("log")?;
        decl.declare("context")?;
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        let (overlay, context) = match ctx.userdata::<RuntimeState>() {
            Some(state) => (state.overlay.clone(), state.context.clone()),
            None => {
                return Err(rquickjs::Exception::throw_message(
                    ctx,
                    "runtime module evaluated without execution state",
                ));
            }
        };

        let workspace = crate::api::fs::build_workspace_object(ctx, overlay.clone())?;
        let log = crate::api::log::build_log_object(ctx, overlay)?;
        let context = ctx.json_parse(context.to_string())?;

        exports.export("workspace", workspace)?;
        exports.export("log", log)?;
        exports.export("context", context)?;
        Ok(())
    }
}
