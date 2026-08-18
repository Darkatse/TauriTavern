//! `$sillytavern`：SillyTavern 运行时代理对象（只读快照）。
//!
//! 当前只实现 `variables` 命名空间，接口签名与
//! `getContext().variables` 保持一致，使 skill 脚本开发者无需学习新 API。
//!
//! ```js
//! $sillytavern.variables.local.get(name)   // 只读
//! $sillytavern.variables.local.has(name)   // boolean
//! $sillytavern.variables.global.get(name)
//! $sillytavern.variables.global.has(name)
//! ```
//!
//! 写操作（`set` / `del` / `add` / `inc` / `dec`）存在但会抛出
//! "variables are read-only in skill script sandbox" 错误，
//! 以保持接口签名一致并 fail-fast 提示开发者。

use rquickjs::{Ctx, Function, Object};
use serde_json::Value;

use tt_domain::models::skill_script::SillyTavernVariableSnapshot;

use crate::convert::json_to_js;

/// 将 `$sillytavern` 全局代理对象注入 JS context。
///
/// 结构与 SillyTavern extension API 一致：
/// ```js
/// $sillytavern.getContext().variables.local.get(name)
/// $sillytavern.getContext().variables.global.has(name)
/// ```
/// 沙箱内 `getContext()` 每次返回同一份冻结快照。
pub(crate) fn register_sillytavern_api<'js>(
    ctx: &Ctx<'js>,
    snapshot: SillyTavernVariableSnapshot,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let sillytavern = Object::new(ctx.clone())?;

    // 预建 context 对象（含 variables），存为 sillytavern 的内部属性。
    // getContext() 从该属性返回冻结快照，不捕获 'js 生命周期的 handle。
    let context = build_context_object(ctx, snapshot)?;
    sillytavern.set("_context", context)?;

    // getContext() — 返回预建的 context 对象。
    let get_context_fn = Function::new(
        ctx.clone(),
        move |this: rquickjs::Object<'js>| -> rquickjs::Result<rquickjs::Value<'js>> {
            this.get::<_, rquickjs::Value<'js>>("_context")
        },
    )?;
    sillytavern.set("getContext", get_context_fn)?;

    globals.set("$sillytavern", sillytavern)?;
    Ok(())
}

/// 构建 `getContext()` 返回的 context 对象。当前只含 `variables` 命名空间，
/// 未来可扩展 lorebook、chatMessages 等。
fn build_context_object<'js>(
    ctx: &Ctx<'js>,
    snapshot: SillyTavernVariableSnapshot,
) -> rquickjs::Result<rquickjs::Object<'js>> {
    let context = Object::new(ctx.clone())?;
    let variables = build_variables_namespace(ctx, snapshot)?;
    context.set("variables", variables)?;
    Ok(context)
}

fn build_variables_namespace<'js>(
    ctx: &Ctx<'js>,
    snapshot: SillyTavernVariableSnapshot,
) -> rquickjs::Result<Object<'js>> {
    let variables = Object::new(ctx.clone())?;

    let local = build_variable_scope(ctx, snapshot.local)?;
    let global = build_variable_scope(ctx, snapshot.global)?;

    variables.set("local", local)?;
    variables.set("global", global)?;
    Ok(variables)
}

/// 构建单个变量作用域（`local` 或 `global`），接口签名与
/// `getContext().variables.{local,global}` 一致。
fn build_variable_scope<'js>(
    ctx: &Ctx<'js>,
    map: serde_json::Map<String, Value>,
) -> rquickjs::Result<Object<'js>> {
    let scope = Object::new(ctx.clone())?;

    // get(name) — 返回原始值，缺失时返回空字符串（与 ST getLocalVariable 行为一致）。
    let get_map = map.clone();
    let get_fn = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, name: String| {
            let value = get_map.get(&name).cloned().unwrap_or(Value::String(String::new()));
            json_to_js(&ctx, &value)
        },
    )?;
    scope.set("get", get_fn)?;

    // has(name) — 返回 boolean（与 ST existsLocalVariable 一致）。
    let has_map = map.clone();
    let has_fn = Function::new(
        ctx.clone(),
        move |name: String| has_map.contains_key(&name),
    )?;
    scope.set("has", has_fn)?;

    // set / del / add / inc / dec — 只读模式下 fail-fast。
    let readonly_error = "variables are read-only in skill script sandbox";
    for method in ["set", "del", "add", "inc", "dec"] {
        let error_msg = readonly_error.to_string();
        let fn_ = Function::new(ctx.clone(), move |_ctx: Ctx<'js>| -> rquickjs::Result<Value> {
            Err(rquickjs::Error::new_string(error_msg.clone()))
        })?;
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

    fn run_with_snapshot<F>(snapshot: SillyTavernVariableSnapshot, body: F)
    where
        F: FnOnce(&Ctx),
    {
        let runtime = Runtime::new().expect("runtime");
        let context = Context::full(&runtime).expect("context");
        context.with(|ctx| {
            register_sillytavern_api(&ctx, snapshot).expect("register");
            body(&ctx);
        });
    }

    #[test]
    fn local_get_returns_value_by_name() {
        let mut local = serde_json::Map::new();
        local.insert("score".to_string(), json!(42));
        local.insert("name".to_string(), json!("Alice"));
        let snapshot = SillyTavernVariableSnapshot {
            local,
            global: serde_json::Map::new(),
        };

        run_with_snapshot(snapshot, |ctx| {
            let result = ctx
                .eval::<rquickjs::Value, _>("$sillytavern.getContext().variables.local.get('score')")
                .expect("eval");
            assert_eq!(js_to_json(ctx, &result).unwrap(), json!(42));

            let result = ctx
                .eval::<rquickjs::Value, _>("$sillytavern.getContext().variables.local.get('name')")
                .expect("eval");
            assert_eq!(js_to_json(ctx, &result).unwrap(), json!("Alice"));
        });
    }

    #[test]
    fn local_get_returns_empty_string_for_missing() {
        let snapshot = SillyTavernVariableSnapshot::default();

        run_with_snapshot(snapshot, |ctx| {
            let result = ctx
                .eval::<rquickjs::Value, _>("$sillytavern.getContext().variables.local.get('missing')")
                .expect("eval");
            assert_eq!(js_to_json(ctx, &result).unwrap(), json!(""));
        });
    }

    #[test]
    fn local_has_returns_boolean() {
        let mut local = serde_json::Map::new();
        local.insert("exists".to_string(), json!("yes"));
        let snapshot = SillyTavernVariableSnapshot {
            local,
            global: serde_json::Map::new(),
        };

        run_with_snapshot(snapshot, |ctx| {
            let result = ctx
                .eval::<bool, _>("$sillytavern.getContext().variables.local.has('exists')")
                .expect("eval");
            assert!(result);

            let result = ctx
                .eval::<bool, _>("$sillytavern.getContext().variables.local.has('nope')")
                .expect("eval");
            assert!(!result);
        });
    }

    #[test]
    fn global_get_and_has_work() {
        let mut global = serde_json::Map::new();
        global.insert("theme".to_string(), json!("dark"));
        let snapshot = SillyTavernVariableSnapshot {
            local: serde_json::Map::new(),
            global,
        };

        run_with_snapshot(snapshot, |ctx| {
            let result = ctx
                .eval::<rquickjs::Value, _>("$sillytavern.getContext().variables.global.get('theme')")
                .expect("eval");
            assert_eq!(js_to_json(ctx, &result).unwrap(), json!("dark"));

            let result = ctx
                .eval::<bool, _>("$sillytavern.getContext().variables.global.has('theme')")
                .expect("eval");
            assert!(result);
        });
    }

    #[test]
    fn write_operations_fail_fast() {
        let snapshot = SillyTavernVariableSnapshot::default();

        run_with_snapshot(snapshot, |ctx| {
            let result = ctx.eval::<rquickjs::Value, _>(
                "$sillytavern.getContext().variables.local.set('x', 1)",
            );
            assert!(result.is_err());

            let result = ctx.eval::<rquickjs::Value, _>(
                "$sillytavern.getContext().variables.global.del('x')",
            );
            assert!(result.is_err());

            let result = ctx.eval::<rquickjs::Value, _>(
                "$sillytavern.getContext().variables.local.inc('x')",
            );
            assert!(result.is_err());
        });
    }

    #[test]
    fn string_values_preserved_without_number_coercion() {
        let mut local = serde_json::Map::new();
        local.insert("count".to_string(), json!("42"));
        let snapshot = SillyTavernVariableSnapshot {
            local,
            global: serde_json::Map::new(),
        };

        run_with_snapshot(snapshot, |ctx| {
            let result = ctx
                .eval::<rquickjs::Value, _>("$sillytavern.getContext().variables.local.get('count')")
                .expect("eval");
            // 应该返回字符串 "42"，不是数字 42
            assert_eq!(js_to_json(ctx, &result).unwrap(), json!("42"));
        });
    }

    #[test]
    fn get_context_returns_same_object_each_call() {
        let snapshot = SillyTavernVariableSnapshot::default();

        run_with_snapshot(snapshot, |ctx| {
            let result = ctx
                .eval::<bool, _>("$sillytavern.getContext() === $sillytavern.getContext()")
                .expect("eval");
            assert!(result, "getContext() should return the same object reference");
        });
    }
}
