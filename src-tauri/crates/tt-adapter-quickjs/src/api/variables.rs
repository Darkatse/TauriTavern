//! `context.variables`：SillyTavern 变量快照（只读，纯 JSON 输入）。
//!
//! 应用层将变量快照投影为
//! `{ "local": { ... }, "global": { ... } }` 格式的纯 JSON 后传入，
//! 适配器不依赖任何领域模型类型。
//!
//! ```js
//! context.variables.local.get(name)   // 只读，缺失返回 ""
//! context.variables.local.has(name)   // boolean
//! context.variables.global.get(name)
//! context.variables.global.has(name)
//! ```
//!
//! 写操作（`set` / `del` / `add` / `inc` / `dec`）fail-fast 抛错。

use rquickjs::{Ctx, Function, Object};
use serde_json::Value;

use crate::convert::json_to_js;

/// 构建 `variables` 对象：local / global 两个只读 scope。
/// 由 `@tauritavern/runtime/v1` 原生模块的 `context` 导出，不再注入全局。
///
/// `snapshot_json` 应为 `{ "local": { ... }, "global": { ... } }` 格式。
pub(crate) fn build_variables_object<'js>(
    ctx: &Ctx<'js>,
    snapshot_json: Value,
) -> rquickjs::Result<Object<'js>> {
    let variables = Object::new(ctx.clone())?;

    let local = snapshot_json
        .get("local")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));
    let global = snapshot_json
        .get("global")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    let local_scope = build_variable_scope(ctx, local)?;
    let global_scope = build_variable_scope(ctx, global)?;

    variables.set("local", local_scope)?;
    variables.set("global", global_scope)?;
    Ok(variables)
}

fn build_variable_scope<'js>(
    ctx: &Ctx<'js>,
    map: Value,
) -> rquickjs::Result<Object<'js>> {
    let scope = Object::new(ctx.clone())?;

    // get(name) — 返回原始值，缺失时返回空字符串
    let get_map = map.clone();
    let get_fn = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, name: String| {
            let value = get_map
                .get(&name)
                .cloned()
                .filter(|v| !v.is_null())
                .unwrap_or(Value::String(String::new()));
            json_to_js(&ctx, &value)
        },
    )?;
    scope.set("get", get_fn)?;

    // has(name) — 返回 boolean
    let has_map = map.clone();
    let has_fn = Function::new(
        ctx.clone(),
        move |name: String| -> bool {
            has_map.get(&name).is_some()
        },
    )?;
    scope.set("has", has_fn)?;

    // set / del / add / inc / dec — fail-fast readonly
    let readonly_error = "variables are read-only in skill script sandbox";
    for method in ["set", "del", "add", "inc", "dec"] {
        let error_msg = readonly_error.to_string();
        let fn_ = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>| -> Result<rquickjs::Value, rquickjs::Error> {
                Err(rquickjs::Exception::throw_message(&ctx, &error_msg))
            },
        )?;
        scope.set(method, fn_)?;
    }

    Ok(scope)
}
