use serde_json::Value;

pub(super) fn render_markdown_value(value: &Value, indent: usize) -> String {
    let prefix = " ".repeat(indent);
    match value {
        Value::Null => format!("{prefix}_None_"),
        Value::Bool(value) => format!("{prefix}{value}"),
        Value::Number(value) => format!("{prefix}{value}"),
        Value::String(value) => value
            .trim()
            .lines()
            .map(|line| format!("{prefix}{line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Array(items) => {
            if items.is_empty() {
                return format!("{prefix}_None provided._");
            }
            items
                .iter()
                .map(|item| {
                    if let Some(inline) = inline_value(item) {
                        format!("{prefix}- {inline}")
                    } else {
                        format!("{prefix}-\n{}", render_markdown_value(item, indent + 2))
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Value::Object(object) => {
            if object.is_empty() {
                return format!("{prefix}_None provided._");
            }
            object
                .iter()
                .map(|(key, value)| {
                    if let Some(inline) = inline_value(value) {
                        format!("{prefix}- **{key}**: {inline}")
                    } else {
                        format!(
                            "{prefix}- **{key}**:\n{}",
                            render_markdown_value(value, indent + 2)
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

pub(super) fn render_inline_value(value: &Value) -> String {
    inline_value(value).unwrap_or_else(|| render_markdown_value(value, 0))
}

pub(super) fn indent_lines(text: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn inline_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("_None_".to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) if !value.contains('\n') => Some(value.trim().to_string()),
        Value::String(_) | Value::Array(_) | Value::Object(_) => None,
    }
}
