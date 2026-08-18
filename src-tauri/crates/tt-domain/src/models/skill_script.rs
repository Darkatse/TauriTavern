//! 模型：skill 脚本执行所需的运行时快照数据。

use serde_json::Value;

/// 当前 agent run 激活的世界书条目（预取自 run prompt snapshot），
/// 经 `$worldInfo` API 提供给 skill 脚本。
#[derive(Debug, Clone)]
pub struct ActivatedWorldInfoEntry {
    pub world: String,
    pub uid: String,
    pub display_name: Option<String>,
    pub constant: bool,
    pub position: Option<String>,
    pub content: String,
    pub ref_key: String,
}

impl ActivatedWorldInfoEntry {
    /// 从 `worldInfoActivation.entries[i]` JSON 项解析；
    /// 非对象或缺 string content 时返回 `None`（调用方跳过）。
    pub fn from_value(index: usize, value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let world = object
            .get("world")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let uid = match object.get("uid") {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => String::new(),
        };
        let ref_key = if world.is_empty() || uid.is_empty() {
            format!("worldinfo:activated#{index}")
        } else {
            format!("worldinfo:{world}#{uid}")
        };
        let content = object.get("content")?.as_str()?.to_string();
        Some(Self {
            display_name: object
                .get("displayName")
                .and_then(Value::as_str)
                .map(String::from),
            constant: object
                .get("constant")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            position: object
                .get("position")
                .and_then(Value::as_str)
                .map(String::from),
            world,
            uid,
            content,
            ref_key,
        })
    }
}

/// SillyTavern 变量快照（预取自 run prompt snapshot），经 `$sillytavern.variables`
/// API 提供给 skill 脚本。变量在 run 开始时冻结，脚本内只读。
///
/// `local` 对应 `chat_metadata.variables`，`global` 对应
/// `extension_settings.variables.global`。两者均为 `name → value` 的 flat map，
/// 值保持原始类型（不做 ST 的 `Number()` 转换）。
#[derive(Debug, Clone, Default)]
pub struct SillyTavernVariableSnapshot {
    pub local: serde_json::Map<String, Value>,
    pub global: serde_json::Map<String, Value>,
}

impl SillyTavernVariableSnapshot {
    /// 从 `promptSnapshot.variables` JSON 项解析。
    /// 预期格式：`{ "local": { ... }, "global": { ... } }`。
    /// 缺失的 map 回退为空；非对象字段跳过（不 fail）。
    pub fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        let local = object
            .and_then(|map| map.get("local"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let global = object
            .and_then(|map| map.get("global"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        Self { local, global }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivatedWorldInfoEntry, SillyTavernVariableSnapshot};
    use serde_json::json;

    #[test]
    fn parses_entry_and_builds_ref_key() {
        let entry = ActivatedWorldInfoEntry::from_value(
            0,
            &json!({
                "world": "lore",
                "uid": "12",
                "displayName": "Capital",
                "constant": true,
                "position": "before",
                "content": "The capital is Alden."
            }),
        )
        .expect("valid entry");

        assert_eq!(entry.ref_key, "worldinfo:lore#12");
        assert_eq!(entry.content, "The capital is Alden.");
        assert!(entry.constant);
        assert_eq!(entry.position.as_deref(), Some("before"));
    }

    #[test]
    fn numeric_uid_is_stringified() {
        let entry = ActivatedWorldInfoEntry::from_value(
            0,
            &json!({ "world": "lore", "uid": 7, "content": "text" }),
        )
        .expect("valid entry");

        assert_eq!(entry.ref_key, "worldinfo:lore#7");
    }

    #[test]
    fn missing_content_or_object_returns_none() {
        assert!(ActivatedWorldInfoEntry::from_value(0, &json!("plain")).is_none());
        assert!(ActivatedWorldInfoEntry::from_value(0, &json!({ "world": "lore" })).is_none());
    }

    #[test]
    fn missing_world_falls_back_to_indexed_ref() {
        let entry = ActivatedWorldInfoEntry::from_value(3, &json!({ "content": "x" }))
            .expect("valid entry");

        assert_eq!(entry.ref_key, "worldinfo:activated#3");
    }

    #[test]
    fn variable_snapshot_parses_local_and_global() {
        let snapshot = SillyTavernVariableSnapshot::from_value(&json!({
            "local": { "score": 42, "name": "Alice" },
            "global": { "theme": "dark" }
        }));

        assert_eq!(snapshot.local.get("score"), Some(&json!(42)));
        assert_eq!(snapshot.local.get("name"), Some(&json!("Alice")));
        assert_eq!(snapshot.global.get("theme"), Some(&json!("dark")));
    }

    #[test]
    fn variable_snapshot_defaults_missing_maps_to_empty() {
        let snapshot = SillyTavernVariableSnapshot::from_value(&json!({}));

        assert!(snapshot.local.is_empty());
        assert!(snapshot.global.is_empty());
    }

    #[test]
    fn variable_snapshot_handles_null_input() {
        let snapshot = SillyTavernVariableSnapshot::from_value(&Value::Null);

        assert!(snapshot.local.is_empty());
        assert!(snapshot.global.is_empty());
    }

    #[test]
    fn variable_snapshot_preserves_string_values_without_number_coercion() {
        // ST 的 getLocalVariable 会把 "42" 转成 42；快照保持原始值。
        let snapshot = SillyTavernVariableSnapshot::from_value(&json!({
            "local": { "count": "42" }
        }));

        assert_eq!(snapshot.local.get("count"), Some(&json!("42")));
    }
}
