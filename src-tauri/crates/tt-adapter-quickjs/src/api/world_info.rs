//! `$worldInfo`：读取预取的激活世界书快照（只读）。

use rquickjs::{Ctx, Function, Object};
use serde::Serialize;
use serde_json::json;

use tt_domain::models::skill_script::ActivatedWorldInfoEntry;

use crate::convert::json_to_js;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScriptWorldInfoEntry<'a> {
    uid: &'a str,
    #[serde(rename = "ref")]
    ref_key: &'a str,
    content: &'a str,
    constant: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<&'a str>,
    world: &'a str,
}

fn entries_json(entries: &[ActivatedWorldInfoEntry]) -> serde_json::Value {
    json!({
        "entries": entries
            .iter()
            .map(|entry| serde_json::to_value(ScriptWorldInfoEntry {
                uid: entry.uid.as_str(),
                ref_key: entry.ref_key.as_str(),
                content: entry.content.as_str(),
                constant: entry.constant,
                position: entry.position.as_deref(),
                display_name: entry.display_name.as_deref(),
                world: entry.world.as_str(),
            })
            .unwrap_or(serde_json::Value::Null))
            .collect::<Vec<_>>(),
    })
}

pub(crate) fn register_world_info_api<'js>(
    ctx: &Ctx<'js>,
    entries: Vec<ActivatedWorldInfoEntry>,
) -> rquickjs::Result<()> {
    let globals = ctx.globals();
    let object = Object::new(ctx.clone())?;

    let activated = entries.clone();
    let read_activated = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>| json_to_js(&ctx, &entries_json(&activated)),
    )?;

    let filtered = entries;
    let read_entries = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, refs: Vec<String>| {
            let selected: Vec<_> = filtered
                .iter()
                .filter(|entry| refs.contains(&entry.ref_key))
                .cloned()
                .collect();
            json_to_js(&ctx, &entries_json(&selected))
        },
    )?;

    object.set("readActivated", read_activated)?;
    object.set("readEntries", read_entries)?;
    globals.set("$worldInfo", object)?;
    Ok(())
}
