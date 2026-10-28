//! Replays the frontend perf suite's host calls through the command layer on Tauri's mock runtime, over a fresh
//! copy of the suite's data root, and reports what each measured step cost. Driven by tests/perf/backend.mjs.

mod counters;
mod environment;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::http::{HeaderMap, HeaderName, HeaderValue};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response};
use tauri::webview::InvokeRequest;
use tauri::{WebviewWindow, WebviewWindowBuilder};

#[derive(Deserialize)]
struct Trace {
    calls: Vec<Call>,
}

/// One host call as the frontend made it. `args` holds `{ "$bytes": base64 }` for a binary body, and
/// `{ "$result": [call, path] }` wherever the call passed a value an earlier call returned.
#[derive(Deserialize)]
struct Call {
    step: Option<String>,
    command: String,
    args: Value,
    headers: Value,
}

/// Calls whose work lies outside the backend's code: Tauri plugins, the host shell's startup handshake, callbacks
/// into the page, the network and the OS.
fn replayed(command: &str) -> bool {
    !command.starts_with("plugin:")
        && !matches!(
            command,
            "backend_error_bridge_ready"
                | "wait_for_backend_ready"
                | "register_agent_extension_tool"
                | "check_for_update"
                | "get_chat_completions_status"
                | "start_chat_completion_stream"
                | "read_chat_completion_stream"
                | "close_chat_completion_stream"
                | "get_notification_permission_state"
        )
}

struct OpenStep {
    name: String,
    before: counters::Counters,
    result_bytes: u64,
}

pub fn run() -> Result<(), String> {
    let [data, trace, report, repo]: [String; 4] = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| {
            "usage: tt-perf <data-root> <trace.json> <report.json> <repo-root>".to_string()
        })?;
    let trace: Trace = serde_json::from_slice(
        &std::fs::read(&trace).map_err(|error| format!("{trace}: {error}"))?,
    )
    .map_err(|error| format!("{trace}: {error}"))?;
    // One worker and one blocking thread: per-thread caches then warm the same way on every run.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::set(runtime.handle().clone());
    let root = std::env::temp_dir().join(format!("tt-perf-{}", std::process::id()));
    copy_dir(Path::new(&data), &root.join("data"))?;
    let outcome = replay(&trace, &root, Path::new(&repo));
    std::fs::remove_dir_all(&root).map_err(|error| format!("{}: {error}", root.display()))?;
    let (steps, boundaries) = outcome?;
    std::fs::write(
        &report,
        json!({ "steps": steps, "boundaries": boundaries }).to_string(),
    )
    .map_err(|error| format!("{report}: {error}"))
}

fn replay(
    trace: &Trace,
    root: &Path,
    repo: &Path,
) -> Result<(BTreeMap<String, counters::Counters>, Vec<Value>), String> {
    let app = environment::mock_app(root, repo)?;
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .map_err(|error| error.to_string())?;
    let referenced = referenced_calls(&trace.calls);
    let mut results = BTreeMap::new();
    let mut steps = BTreeMap::new();
    let mut boundaries = Vec::new();
    let mut open: Option<OpenStep> = None;
    for (index, call) in trace.calls.iter().enumerate() {
        if open.as_ref().map(|step| &step.name) != call.step.as_ref() {
            if let Some(step) = open.take() {
                close_step(step, &mut steps, &mut boundaries)?;
            }
            if let Some(name) = &call.step {
                // Callgrind writes a dump at each boundary, so the I/O window lies inside them.
                counters::tt_perf_step_boundary();
                boundaries.push(json!({ "step": name, "edge": "start" }));
                let before = counters::snapshot()?;
                open = Some(OpenStep {
                    name: name.clone(),
                    before,
                    result_bytes: 0,
                });
            }
        }
        if !replayed(&call.command) {
            continue;
        }
        let response = invoke(&window, call, &results)
            .map_err(|error| format!("call {index} ({}): {error}", call.command))?;
        if let Some(step) = &mut open {
            step.result_bytes += match &response {
                InvokeResponseBody::Json(json) => json.len(),
                InvokeResponseBody::Raw(raw) => raw.len(),
            } as u64;
        }
        if referenced.contains(&index) {
            results.insert(
                index,
                parsed(response).map_err(|error| format!("call {index}: {error}"))?,
            );
        }
    }
    if let Some(step) = open.take() {
        close_step(step, &mut steps, &mut boundaries)?;
    }
    Ok((steps, boundaries))
}

fn close_step(
    step: OpenStep,
    steps: &mut BTreeMap<String, counters::Counters>,
    boundaries: &mut Vec<Value>,
) -> Result<(), String> {
    let mut work = counters::difference(&counters::snapshot()?, &step.before);
    counters::tt_perf_step_boundary();
    boundaries.push(json!({ "step": step.name, "edge": "end" }));
    work.insert("resultBytes", step.result_bytes);
    if steps.insert(step.name.clone(), work).is_some() {
        return Err(format!("step {} occurs twice in the trace", step.name));
    }
    Ok(())
}

fn invoke(
    webview: &WebviewWindow<MockRuntime>,
    call: &Call,
    results: &BTreeMap<usize, Value>,
) -> Result<InvokeResponseBody, String> {
    let body = match resolve(&call.args, results)? {
        Value::Object(fields) if fields.len() == 1 && fields.contains_key("$bytes") => {
            InvokeBody::Raw(
                base64::engine::general_purpose::STANDARD
                    .decode(fields["$bytes"].as_str().ok_or("$bytes is not a string")?)
                    .map_err(|error| error.to_string())?,
            )
        }
        args => InvokeBody::Json(args),
    };
    let mut headers = HeaderMap::new();
    if let Value::Object(fields) = resolve(&call.headers, results)? {
        for (name, value) in fields {
            let value = value
                .as_str()
                .ok_or_else(|| format!("header {name} is not a string"))?;
            headers.insert(
                HeaderName::from_bytes(name.as_bytes()).map_err(|error| error.to_string())?,
                HeaderValue::from_str(value).map_err(|error| error.to_string())?,
            );
        }
    }
    get_ipc_response(
        webview,
        InvokeRequest {
            cmd: call.command.clone(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: webview.url().map_err(|error| error.to_string())?,
            body,
            headers,
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map_err(|error| error.to_string())
}

/// A binary result is the JSON document the page parses from it, as the trace recorded it.
fn parsed(response: InvokeResponseBody) -> serde_json::Result<Value> {
    match response {
        InvokeResponseBody::Json(json) => serde_json::from_str(&json),
        InvokeResponseBody::Raw(raw) => serde_json::from_slice(&raw),
    }
}

fn reference(value: &Value) -> Option<(usize, Vec<String>)> {
    match value {
        Value::Object(fields) if fields.len() == 1 => {
            serde_json::from_value(fields.get("$result")?.clone()).ok()
        }
        _ => None,
    }
}

fn resolve(value: &Value, results: &BTreeMap<usize, Value>) -> Result<Value, String> {
    if let Some((call, path)) = reference(value) {
        let mut current = results
            .get(&call)
            .ok_or_else(|| format!("call {call} returned nothing to refer to"))?;
        for key in &path {
            current = match current {
                Value::Array(items) => key.parse::<usize>().ok().and_then(|index| items.get(index)),
                Value::Object(fields) => fields.get(key),
                _ => None,
            }
            .ok_or_else(|| format!("call {call} returned no {path:?}"))?;
        }
        return Ok(current.clone());
    }
    Ok(match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, item)| Ok((key.clone(), resolve(item, results)?)))
                .collect::<Result<_, String>>()?,
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| resolve(item, results))
                .collect::<Result<_, _>>()?,
        ),
        other => other.clone(),
    })
}

fn referenced_calls(calls: &[Call]) -> HashSet<usize> {
    fn collect(value: &Value, found: &mut HashSet<usize>) {
        if let Some((call, _)) = reference(value) {
            found.insert(call);
        } else if let Value::Object(fields) = value {
            fields.values().for_each(|item| collect(item, found));
        } else if let Value::Array(items) = value {
            items.iter().for_each(|item| collect(item, found));
        }
    }
    let mut found = HashSet::new();
    for call in calls {
        collect(&call.args, &mut found);
        collect(&call.headers, &mut found);
    }
    found
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let target = to.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .map_err(|error| format!("{}: {error}", target.display()))?;
        }
    }
    Ok(())
}
