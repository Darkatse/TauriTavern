//! `$variables`：SillyTavern 变量快照（只读，纯 JSON 输入）。
//!
//! 应用层将变量快照投影为
//! `{ "local": { ... }, "global": { ... } }` 格式的纯 JSON 后传入，
//! 适配器不依赖任何领域模型类型。
//!
//! ```js
//! $variables.local.get(name)   // 只读，缺失返回 ""
//! $variables.local.has(name)   // boolean
//! $variables.global.get(name)
//! $variables.global.has(name)
//! ```
//!
//! 写操作（`set` / `del` / `add` / `inc` / `dec`）fail-fast 抛错。

use rquickjs::{Ctx, Function, Object};
use serde_json::Value;

use crate::convert::json_to_js;

/// 将 `$variables` 全局对象注入 JS context。
///
/// `snapshot_json` 应为 `{ "local": { ... }, "global": { ... } }` 格式。
pub(crate) fn register_variables_api<'js>(
    ctx: &Ctx<'js>,
    snapshot_json: Value,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
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
    globals.set("$variables", variables)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::js_to_json;
    use rquickjs::{Context, Runtime};
    use serde_json::json;

    fn run_with_snapshot<F>(snapshot: Value, body: F)
    where
        F: FnOnce(&Ctx),
    {
        let runtime = Runtime::new().expect("runtime");
        let context = Context::full(&runtime).expect("context");
        context.with(|ctx| {
            register_variables_api(&ctx, snapshot).expect("register");
            body(&ctx);
        });
    }

    #[test]
    fn local_get_returns_value_by_name() {
        let snapshot = json!({
            "local": { "score": 42, "name": "Alice" },
            "global": {}
        });
        run_with_snapshot(snapshot, |ctx| {
            let result = ctx
                .eval::<rquickjs::Value, _>("$variables.local.get('score')")
                .expect("eval");
            assert_eq!(js_to_json(ctx, &result).unwrap(), json!(42));
        });
    }

    #[test]
    fn local_get_returns_empty_string_for_missing() {
        run_with_snapshot(json!({ "local": {}, "global": {} }), |ctx| {
            let result = ctx
                .eval::<rquickjs::Value, _>("$variables.local.get('missing')")
                .expect("eval");
            assert_eq!(js_to_json(ctx, &result).unwrap(), json!(""));
        });
    }

    #[test]
    fn write_operations_fail_fast() {
        run_with_snapshot(json!({ "local": {}, "global": {} }), |ctx| {
            assert!(ctx
                .eval::<rquickjs::Value, _>("$variables.local.set('x', 1)")
                .is_err());
        });
    }
}
