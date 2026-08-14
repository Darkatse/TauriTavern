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

#[cfg(test)]
mod tests {
    use super::ActivatedWorldInfoEntry;
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
}
