//! Replays the frontend perf suite's host calls through the command layer on Tauri's mock runtime and reports what
//! each measured step cost: `prepare` replays the previous launch over the data, and `measure` the rest.

mod allocator;
mod counters;
mod environment;

pub use allocator::Allocator;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::WebviewWindow;
use tauri::http::{HeaderMap, HeaderName, HeaderValue};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeError, InvokeResponse, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime};
use tauri::webview::InvokeRequest;

const USAGE: &str = "usage: tt-perf prepare <app-root> <trace.json> <repo-root>
       tt-perf measure <app-root> <trace.json> <report.json> <repo-root>";

/// 2026-01-01T00:00:00Z.
const FILE_TIME: Duration = Duration::from_secs(1_767_225_600);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Trace {
    previous_launch: Vec<Call>,
    /// Starts with the measured launch's step.
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

/// Skips calls whose work lies outside the backend's code: Tauri plugins, the host shell's startup handshake,
/// callbacks into the page, the network and the OS.
fn replayed(call: &Call) -> bool {
    !call.command.starts_with("plugin:")
        && !matches!(
            call.command.as_str(),
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

pub fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    // Commands run on this thread while it awaits their responses, so a call's work is done when the call returns.
    // One blocking thread serves the whole replay, so per-thread caches warm the same way on every run. Without an
    // I/O driver, waking this thread takes no eventfd write for the I/O counters to count.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(1)
        .thread_keep_alive(Duration::MAX)
        .enable_time()
        .build()
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::set(runtime.handle().clone());
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["prepare", root, trace, repo] => runtime.block_on(prepare(
            Path::new(root),
            &read_trace(trace)?,
            Path::new(repo),
        )),
        ["measure", root, trace, report, repo] => {
            let (steps, boundaries) = runtime.block_on(measure(
                Path::new(root),
                &read_trace(trace)?,
                Path::new(repo),
            ))?;
            std::fs::write(
                report,
                json!({ "steps": steps, "boundaries": boundaries }).to_string(),
            )
            .map_err(|error| format!("{report}: {error}"))
        }
        _ => Err(USAGE.to_string()),
    }
}

fn read_trace(path: &str) -> Result<Trace, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|error| format!("{path}: {error}"))?)
        .map_err(|error| format!("{path}: {error}"))
}

/// Every data file first takes the same modification time, so what the launch derives from file times is the same
/// on every machine.
async fn prepare(root: &Path, trace: &Trace, repo: &Path) -> Result<(), String> {
    let (app, window) = environment::mock_app(repo)?;
    environment::start_backend(&app, root).await?;
    set_file_times(&root.join("data"))?;
    replay(&window, &trace.previous_launch, None)
        .await
        .map_err(|error| format!("previous launch: {error}"))
}

/// The launch's step opens before the backend starts.
async fn measure(
    root: &Path,
    trace: &Trace,
    repo: &Path,
) -> Result<(BTreeMap<String, counters::Counters>, Vec<Value>), String> {
    let (app, window) = environment::mock_app(repo)?;
    let mut steps = Steps::default();
    steps.enter(trace.calls.first().and_then(|call| call.step.as_deref()))?;
    environment::start_backend(&app, root).await?;
    replay(&window, &trace.calls, Some(&mut steps)).await?;
    steps.finish()
}

async fn replay(
    window: &WebviewWindow<MockRuntime>,
    calls: &[Call],
    mut steps: Option<&mut Steps>,
) -> Result<(), String> {
    let referenced = referenced_calls(calls);
    let mut results = BTreeMap::new();
    for (index, call) in calls.iter().enumerate() {
        if let Some(steps) = steps.as_deref_mut() {
            steps.enter(call.step.as_deref())?;
        }
        if !replayed(call) {
            continue;
        }
        let response = invoke(window, call, &results)
            .await
            .map_err(|error| format!("call {index} ({}): {error}", call.command))?;
        if let Some(steps) = steps.as_deref_mut() {
            steps.count_result(&response);
        }
        if referenced.contains(&index) {
            results.insert(
                index,
                parsed(response).map_err(|error| format!("call {index}: {error}"))?,
            );
        }
    }
    Ok(())
}

fn set_file_times(dir: &Path) -> Result<(), String> {
    let failed = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    for entry in std::fs::read_dir(dir).map_err(|error| failed(dir, error))? {
        let path = entry.map_err(|error| failed(dir, error))?.path();
        if path.is_dir() {
            set_file_times(&path)?;
        } else {
            std::fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(UNIX_EPOCH + FILE_TIME))
                .map_err(|error| failed(&path, error))?;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Steps {
    counted: BTreeMap<String, counters::Counters>,
    boundaries: Vec<Value>,
    open: Option<OpenStep>,
}

struct OpenStep {
    name: String,
    before: counters::Snapshot,
    result_bytes: u64,
}

impl Steps {
    fn enter(&mut self, step: Option<&str>) -> Result<(), String> {
        if self.open.as_ref().map(|open| open.name.as_str()) == step {
            return Ok(());
        }
        if let Some(open) = self.open.take() {
            self.close(open)?;
        }
        if let Some(name) = step {
            counters::tt_perf_step_boundary();
            self.boundaries
                .push(json!({ "step": name, "edge": "start" }));
            self.open = Some(OpenStep {
                name: name.to_string(),
                before: counters::snapshot()?,
                result_bytes: 0,
            });
        }
        Ok(())
    }

    fn count_result(&mut self, response: &InvokeResponseBody) {
        if let Some(open) = &mut self.open {
            open.result_bytes += match response {
                InvokeResponseBody::Json(json) => json.len(),
                InvokeResponseBody::Raw(raw) => raw.len(),
            } as u64;
        }
    }

    fn close(&mut self, step: OpenStep) -> Result<(), String> {
        let mut work = counters::since(&step.before)?;
        counters::tt_perf_step_boundary();
        self.boundaries
            .push(json!({ "step": step.name, "edge": "end" }));
        work.insert("resultBytes", step.result_bytes);
        if self.counted.insert(step.name.clone(), work).is_some() {
            return Err(format!("step {} occurs twice in the trace", step.name));
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(BTreeMap<String, counters::Counters>, Vec<Value>), String> {
        if let Some(open) = self.open.take() {
            self.close(open)?;
        }
        Ok((self.counted, self.boundaries))
    }
}

async fn invoke(
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
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview.as_ref().clone().on_message(
        InvokeRequest {
            cmd: call.command.clone(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: webview.url().map_err(|error| error.to_string())?,
            body,
            headers,
            invoke_key: INVOKE_KEY.to_string(),
        },
        Box::new(move |_, _, response, _, _| {
            let _ = sender.send(response);
        }),
    );
    match receiver
        .await
        .map_err(|_| "the command dropped its responder".to_string())?
    {
        InvokeResponse::Ok(body) => Ok(body),
        InvokeResponse::Err(InvokeError(error)) => Err(error.to_string()),
    }
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
