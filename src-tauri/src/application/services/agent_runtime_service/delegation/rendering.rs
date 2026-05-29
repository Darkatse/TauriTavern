use serde_json::Value;

use crate::domain::models::agent::AgentTaskRecord;

pub(super) fn render_child_task_prompt(task: &AgentTaskRecord) -> String {
    let object = task.task.as_object();
    let mut lines = vec![
        "# Delegated Task".to_string(),
        String::new(),
        "You are handling one focused task requested by another Agent.".to_string(),
        "Work only on this task. When finished, call task_return with your result.".to_string(),
        String::new(),
    ];

    push_task_section(
        &mut lines,
        "Title",
        object.and_then(|object| object.get("title")),
    );
    push_task_section(
        &mut lines,
        "Objective",
        object.and_then(|object| object.get("objective")),
    );
    push_task_section(
        &mut lines,
        "Context",
        object.and_then(|object| object.get("context")),
    );
    push_task_section(
        &mut lines,
        "Expected Output",
        object.and_then(|object| object.get("expectedOutput")),
    );

    if let Some(object) = object {
        let extras = object
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "title" | "objective" | "context" | "expectedOutput"
                )
            })
            .collect::<Vec<_>>();
        if !extras.is_empty() {
            lines.push("## Additional Instructions".to_string());
            for (key, value) in extras {
                lines.push(format!("- **{}**:", key));
                lines.push(indent_lines(&render_markdown_value(value, 0), 2));
            }
            lines.push(String::new());
        }
    } else if !task.task.is_null() {
        lines.push("## Task Details".to_string());
        lines.push(render_markdown_value(&task.task, 0));
        lines.push(String::new());
    }

    lines.extend([
        "## Working Notes".to_string(),
        "Use workspace files only when they help complete this task:".to_string(),
        "- summaries/notes.md for durable private notes or a longer capsule".to_string(),
        "- scratch/notes.md for temporary working notes".to_string(),
        "- summaries/parent/ for read-only notes from the Agent that asked for this task, when present"
            .to_string(),
        "- summaries/agents/ for read-only notes from other delegated Agents, when present"
            .to_string(),
        "- shared writable roots only when the task asks for an artifact or edit there"
            .to_string(),
        String::new(),
        "Reference useful note or artifact paths in task_return.".to_string(),
    ]);

    lines.join("\n")
}

pub(super) fn render_task_return_summary(result_doc: &Value) -> String {
    let summary = result_doc
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let task_id = result_doc
        .get("taskId")
        .or_else(|| result_doc.pointer("/runtime/taskId"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let status = result_doc
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("# Delegated Task Result\n\nTask: {task_id}\nStatus: {status}\n\n{summary}\n")
}

pub(super) fn render_await_content(structured: &Value) -> String {
    let timed_out = structured
        .get("timedOut")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tasks = structured
        .get("tasks")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if tasks.is_empty() {
        return "No delegated tasks are selected.".to_string();
    }
    let mut lines = Vec::new();
    lines.push(if timed_out {
        "## Delegated Task Results\n\nTimed out before all selected tasks finished.".to_string()
    } else {
        "## Delegated Task Results".to_string()
    });
    for task in tasks {
        let task_id = task
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let agent_id = task
            .get("agentId")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let status = task
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        lines.push(String::new());
        lines.push(format!("### {agent_id} - {status}"));
        lines.push(format!("Task id: {task_id}"));
        if let Some(error) = task.get("error").and_then(Value::as_str) {
            lines.push(format!("Error: {error}"));
        }
        if let Some(summary) = task.get("summary").and_then(Value::as_str) {
            if !summary.trim().is_empty() {
                lines.push(String::new());
                lines.push(summary.trim().to_string());
            }
        }
        push_optional_result_section(&mut lines, task, "Findings", "findings");
        push_optional_result_section(&mut lines, task, "Warnings", "warnings");
        push_optional_result_section(
            &mut lines,
            task,
            "Suggested Next Actions",
            "suggestedNextActions",
        );
        push_optional_result_section(
            &mut lines,
            task,
            "Questions For Caller",
            "questionsForCaller",
        );
        push_optional_result_section(&mut lines, task, "Artifacts", "artifacts");
        if let Some(confidence) = task.get("confidence") {
            lines.push(format!("Confidence: {}", render_inline_value(confidence)));
        }
    }
    lines.join("\n")
}

fn push_task_section(lines: &mut Vec<String>, title: &str, value: Option<&Value>) {
    let Some(value) = value else {
        return;
    };
    if value.is_null() {
        return;
    }
    if value.as_str().is_some_and(|value| value.trim().is_empty()) {
        return;
    }
    lines.push(format!("## {title}"));
    lines.push(render_markdown_value(value, 0));
    lines.push(String::new());
}

fn push_optional_result_section(lines: &mut Vec<String>, task: &Value, title: &str, key: &str) {
    let Some(value) = task.get(key) else {
        return;
    };
    if value.is_null() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("{title}:"));
    lines.push(indent_lines(&render_markdown_value(value, 0), 2));
}

fn render_markdown_value(value: &Value, indent: usize) -> String {
    if let Some(inline) = inline_value(value) {
        return inline;
    }
    let prefix = " ".repeat(indent);
    match value {
        Value::Array(items) => {
            if items.is_empty() {
                return "_None provided._".to_string();
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
                return "_None provided._".to_string();
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
        _ => render_inline_value(value),
    }
}

fn inline_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("_None_".to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.trim().to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn render_inline_value(value: &Value) -> String {
    inline_value(value).unwrap_or_else(|| render_markdown_value(value, 0))
}

fn indent_lines(text: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
