//! `$worldInfo`：读取预取的激活世界书快照（只读，纯 JSON 输入）。
//!
//! 应用层将激活世界书条目投影为 JSON 后传入，适配器
//! 不依赖任何领域模型类型。

use rquickjs::{Ctx, Function, Object};
use serde_json::Value;

use crate::convert::json_to_js;

/// 将 `$worldInfo` 全局对象注入 JS context。
///
/// `entries_json` 应为 `{ "entries": [...] }` 格式的纯 JSON 值，
/// 由应用层从激活世界书条目列表投影而成。
pub(crate) fn register_world_info_api<'js>(
    ctx: &Ctx<'js>,
    entries_json: Value,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let object = Object::new(ctx.clone())?;

    // readActivated() → 全部激活条目的 JSON 快照
    let activated = entries_json.clone();
    let read_activated = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>| json_to_js(&ctx, &activated),
    )?;

    // readEntries(refs: string[]) → 按 ref 过滤的条目
    let filtered = entries_json;
    let read_entries = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, refs: Vec<String>| -> rquickjs::Result<rquickjs::Value<'js>> {
            let selected = filter_entries_by_refs(&filtered, &refs);
            json_to_js(&ctx, &selected)
        },
    )?;

    object.set("readActivated", read_activated)?;
    object.set("readEntries", read_entries)?;
    globals.set("$worldInfo", object)?;
    Ok(())
}

/// 按 `ref` 字段过滤 `{ "entries": [...] }` 中的条目。
fn filter_entries_by_refs(entries_json: &Value, refs: &[String]) -> Value {
    let Some(entries) = entries_json.get("entries").and_then(Value::as_array) else {
        return serde_json::json!({ "entries": [] });
    };
    let selected: Vec<&Value> = entries
        .iter()
        .filter(|entry| {
            entry
                .get("ref")
                .and_then(Value::as_str)
                .map(|r| refs.contains(&r.to_string()))
                .unwrap_or(false)
        })
        .collect();
    serde_json::json!({ "entries": selected })
}
