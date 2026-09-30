use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use rquickjs::function::{Opt, Rest};
use rquickjs::module::{Declarations, Exports, ModuleDef};
use rquickjs::object::Accessor;
use rquickjs::{Coerced, Ctx, Exception, FromJs, Function, JsLifetime, Object, Result, Value};
use tt_domain::frozen_macros::MAX_EXPANDED_TEXT_BYTES;
use tt_ports::workspace_shell::{
    ChatMessageOutcome, ChatMessageRead, ChatMessageSource, MessageRange, WorkspaceShellContext,
};

use super::files::{Files, workspace_path};

pub(super) const RUNTIME_MODULE: &str = "@tauritavern/runtime";
pub(super) const MAX_OUTPUT_BYTES: usize = crate::engine::MAX_OUTPUT_BYTES;

#[derive(Default)]
pub(super) struct Output {
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn write(&mut self, stderr: bool, text: &str) -> std::result::Result<(), String> {
        let stream = if stderr {
            &mut self.stderr
        } else {
            &mut self.stdout
        };
        if stream.len() + text.len() > MAX_OUTPUT_BYTES {
            return Err(format!(
                "JavaScript output exceeds {MAX_OUTPUT_BYTES} bytes. Save the content to a workspace file and print its path."
            ));
        }
        stream.push_str(text);
        Ok(())
    }
}

pub(super) struct RuntimeState {
    /// Carries the handle the bridges block on, as [`Files::runtime`].
    pub files: Files,
    pub context: Arc<WorkspaceShellContext>,
    pub chat: Option<Arc<dyn ChatMessageSource>>,
    pub output: Rc<RefCell<Output>>,
}

// Host-owned data only; no QuickJS references whose lifetime needs changing.
unsafe impl<'js> JsLifetime<'js> for RuntimeState {
    type Changed<'to> = Self;
}

pub(super) struct RuntimeModule;

impl ModuleDef for RuntimeModule {
    fn declare(exports: &Declarations<'_>) -> Result<()> {
        for name in ["workspace", "context", "macros", "log", "chat"] {
            exports.declare(name)?;
        }
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        let (files, context, chat, runtime, output) = {
            let state = ctx.userdata::<RuntimeState>().ok_or_else(|| {
                Exception::throw_message(ctx, "JavaScript runtime context is missing")
            })?;
            (
                state.files.clone(),
                state.context.clone(),
                state.chat.clone(),
                state.files.runtime.clone(),
                state.output.clone(),
            )
        };
        // The chat bridge charges the shell's input budget exactly as the file
        // bridge does, so it needs the files handle as well as the source. `files`
        // is moved into the `listFiles` closure below, hence the clone here.
        let chat_files = files.clone();
        let workspace = Object::new(ctx.clone())?;
        let read = files.clone();
        workspace.set(
            "readText",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, path: String| {
                workspace_path(&path)
                    .and_then(|path| read.read(&path))
                    .map_err(|message| Exception::throw_message(&ctx, &message))
            })?,
        )?;
        let write = files.clone();
        workspace.set(
            "writeText",
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'_>, path: String, text: String| {
                    workspace_path(&path)
                        .and_then(|path| write.write(&path, &text))
                        .map_err(|message| Exception::throw_message(&ctx, &message))
                },
            )?,
        )?;
        let exists = files.clone();
        workspace.set(
            "exists",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, path: String| {
                workspace_path(&path)
                    .and_then(|path| exists.exists(&path))
                    .map_err(|message| Exception::throw_message(&ctx, &message))
            })?,
        )?;
        workspace.set(
            "listFiles",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, path: Opt<String>| {
                path.0
                    .as_deref()
                    .map(workspace_path)
                    .transpose()
                    .and_then(|path| files.list(path.as_deref()))
                    .map_err(|message| Exception::throw_message(&ctx, &message))
            })?,
        )?;
        exports.export("workspace", workspace)?;

        let host: Value = match &context.host {
            Ok(value) => ctx.json_parse(value.to_string())?,
            Err(_) => {
                let host = Object::new(ctx.clone())?;
                // Missing optional host facts must not disable workspace file APIs.
                for field in ["worldInfo", "variables", "macro"] {
                    let message = format!("context.{field} is unavailable for this task.");
                    host.prop(
                        field,
                        Accessor::from(move |ctx: Ctx<'_>| -> Result<()> {
                            Err(Exception::throw_message(&ctx, &message))
                        })
                        .enumerable(),
                    )?;
                }
                host.into_value()
            }
        };
        exports.export("context", host)?;
        let macros = Object::new(ctx.clone())?;
        macros.set(
            "render",
            Function::new(ctx.clone(), move |ctx: Ctx<'_>, text: String| {
                context
                    .frozen_macros
                    .render(&text, MAX_EXPANDED_TEXT_BYTES)
                    .map(std::borrow::Cow::into_owned)
                    .map_err(|error| Exception::throw_message(&ctx, &error.to_string()))
            })?,
        )?;
        exports.export("macros", macros)?;
        exports.export("chat", chat_object(ctx, chat, chat_files, runtime)?)?;
        exports.export("log", output_object(ctx, output, true)?)?;
        Ok(())
    }
}

pub(super) fn process_object<'js>(
    ctx: &Ctx<'js>,
    argv: Vec<String>,
    exit_code: Rc<Cell<u8>>,
) -> Result<Object<'js>> {
    let process = Object::new(ctx.clone())?;
    process.set("argv", argv)?;
    let current = exit_code.clone();
    process.prop(
        "exitCode",
        Accessor::new(
            move || current.get(),
            move |ctx: Ctx<'js>, value: Value<'js>| -> Result<()> {
                let code = value
                    .as_number()
                    .filter(|code| code.fract() == 0.0 && (0.0..=255.0).contains(code))
                    .ok_or_else(|| {
                        Exception::throw_type(
                            &ctx,
                            "process.exitCode must be a numeric integer from 0 to 255.",
                        )
                    })?;
                exit_code.set(code as u8);
                Ok(())
            },
        )
        .enumerable(),
    )?;
    Ok(process)
}

pub(super) fn output_object<'js>(
    ctx: &Ctx<'js>,
    output: Rc<RefCell<Output>>,
    diagnostics: bool,
) -> Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    for name in ["log", "info", "warn", "error", "debug"] {
        let output = output.clone();
        let stderr = diagnostics || matches!(name, "warn" | "error");
        object.set(
            name,
            Function::new(ctx.clone(), move |ctx: Ctx<'js>, values: Rest<Value<'js>>| {
                let mut line = String::new();
                for (index, value) in values.0.into_iter().enumerate() {
                    let text = Coerced::<String>::from_js(&ctx, value)?.0;
                    if line.len() + text.len() + 2 > MAX_OUTPUT_BYTES {
                        return Err(Exception::throw_message(
                            &ctx,
                            "JavaScript log line exceeds the output limit; write large content to a workspace file.",
                        ));
                    }
                    if index > 0 {
                        line.push(' ');
                    }
                    line.push_str(&text);
                }
                line.push('\n');
                output
                    .borrow_mut()
                    .write(stderr, &line)
                    .map_err(|message| Exception::throw_message(&ctx, &message))
            })?,
        )?;
    }
    Ok(object)
}

/// Build the `chat` namespace: read-only, character-chat-only message access.
fn chat_object<'js>(
    ctx: &Ctx<'js>,
    chat: Option<Arc<dyn ChatMessageSource>>,
    files: Files,
    runtime: tokio::runtime::Handle,
) -> Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;

    let single = chat.clone();
    let single_files = files.clone();
    let single_runtime = runtime.clone();
    object.set(
        "getMessage",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, index: Value<'js>, options: Opt<Value<'js>>| {
                let index = chat_index(&ctx, &index)?;
                let range = chat_range(&ctx, &options.0)?;
                chat_call(
                    &ctx,
                    single.clone(),
                    single_files.clone(),
                    single_runtime.clone(),
                    vec![index],
                    vec![range],
                    false,
                )
            },
        )?,
    )?;

    object.set(
        "getMessages",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, indices: Value<'js>, options: Opt<Value<'js>>| {
                let indices = chat_indices(&ctx, &indices)?;
                let range = chat_range(&ctx, &options.0)?;
                let ranges = vec![range; indices.len()];
                chat_call(
                    &ctx,
                    chat.clone(),
                    files.clone(),
                    runtime.clone(),
                    indices,
                    ranges,
                    true,
                )
            },
        )?,
    )?;

    Ok(object)
}

fn chat_index(ctx: &Ctx<'_>, value: &Value<'_>) -> Result<usize> {
    match value.as_int() {
        Some(index) if index >= 0 => Ok(index as usize),
        _ => Err(Exception::throw_type(
            ctx,
            "chat.getMessage(index) requires a non-negative integer index.",
        )),
    }
}

fn chat_indices(ctx: &Ctx<'_>, value: &Value<'_>) -> Result<Vec<usize>> {
    let array = value.as_array().ok_or_else(|| {
        Exception::throw_type(
            ctx,
            "chat.getMessages(indices) requires an array of non-negative integers.",
        )
    })?;
    let mut indices = Vec::with_capacity(array.len());
    for item in array.iter::<Value<'_>>() {
        let invalid = || {
            Exception::throw_type(
                ctx,
                "chat.getMessages(indices) requires an array of non-negative integers.",
            )
        };
        let item = item.map_err(|_| invalid())?;
        match item.as_int() {
            Some(index) if index >= 0 => indices.push(index as usize),
            _ => return Err(invalid()),
        }
    }
    Ok(indices)
}

fn chat_range(ctx: &Ctx<'_>, options: &Option<Value<'_>>) -> Result<Option<MessageRange>> {
    let Some(options) = options else {
        return Ok(None);
    };
    if options.is_undefined() || options.is_null() {
        return Ok(None);
    }
    let object = options.as_object().ok_or_else(|| {
        Exception::throw_type(
            ctx,
            "chat options must be an object with optional startLine and lineCount.",
        )
    })?;
    let read_int = |key: &str| -> Result<Option<i32>> {
        // A property read that itself fails is a real error, not an absent option;
        // propagate it instead of treating it as unset.
        let value = object.get::<_, Value>(key)?;
        if value.is_undefined() || value.is_null() {
            return Ok(None);
        }
        value.as_int().map(Some).ok_or_else(|| {
            Exception::throw_type(ctx, &format!("chat options.{key} must be an integer."))
        })
    };
    let start_line = read_int("startLine")?;
    let line_count = read_int("lineCount")?;
    if start_line.is_none() && line_count.is_none() {
        return Ok(None);
    }
    let Some(start_line) = start_line else {
        return Err(Exception::throw_type(
            ctx,
            "chat options.startLine is required when lineCount is set.",
        ));
    };
    if start_line < 1 {
        return Err(Exception::throw_type(
            ctx,
            "chat options.startLine must be a positive integer.",
        ));
    }
    let line_count = match line_count {
        None => None,
        Some(count) if count >= 1 => Some(count as usize),
        Some(_) => {
            return Err(Exception::throw_type(
                ctx,
                "chat options.lineCount must be a positive integer.",
            ));
        }
    };
    Ok(Some(MessageRange {
        start_line: start_line as usize,
        line_count,
    }))
}

/// Perform one chat read through the sync-to-async bridge and shape the JS result.
///
/// The bridge blocks the calling thread on `ChatMessageSource::read`, so this must
/// run on a blocking thread. JavaScript execution is driven from `spawn_blocking`,
/// and `runtime` is the handle [`Files`] captured where the shell entered it, not
/// one resolved per call, so an absent runtime is a construction-time problem
/// instead of a mid-script panic.
/// Load errors become JS exceptions (the script can catch them); ordinary lookup
/// and over-limit failures are returned as discriminant objects so the script
/// chooses how to recover. Only malformed arguments throw (see
/// `chat_index`/`chat_indices`/`chat_range` and the empty-list check below).
/// `batch` selects the multi-message result shape used by `getMessages`.
///
/// Chat history is the same resource class as workspace files, so this bridge
/// follows the file bridge exactly: `files.check()` gates the read against the
/// shell's deadline and cancellation, and the text that reaches the script is
/// charged to the shell's aggregate input budget. Both failures are JS exceptions,
/// which is also why a script cannot read more history than it could read files.
fn chat_call<'js>(
    ctx: &Ctx<'js>,
    chat: Option<Arc<dyn ChatMessageSource>>,
    files: Files,
    runtime: tokio::runtime::Handle,
    indices: Vec<usize>,
    ranges: Vec<Option<MessageRange>>,
    batch: bool,
) -> Result<Value<'js>> {
    // Argument validation runs before the capability check, exactly as the index and
    // range parsing in the callers does: a malformed request is a script bug whether
    // or not this run has the chat capability, and reporting it as `chat.unsupported`
    // would hide that. The index list itself is validated there, so only its emptiness
    // is left for here.
    if indices.is_empty() {
        return Err(Exception::throw_type(
            ctx,
            "Provide at least one message index.",
        ));
    }
    let Some(chat) = chat else {
        return unsupported_result(ctx);
    };
    let requests = indices.iter().copied().zip(ranges).collect::<Vec<_>>();

    // A read does not interrupt a scan in progress, so the gate has to sit before
    // the scan starts: once the shell's budget is spent, starting another read
    // would hold `workspace.shell` open past its own deadline.
    files
        .check()
        .map_err(|message| Exception::throw_message(ctx, &message))?;

    let outcome = runtime.block_on(chat.read(&requests));
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            return Err(Exception::throw_message(ctx, &error.to_string()));
        }
    };

    match outcome {
        ChatMessageOutcome::Unsupported => unsupported_result(ctx),
        ChatMessageOutcome::ChatNotFound => {
            let object = Object::new(ctx.clone())?;
            object.set("ok", false)?;
            object.set("reason", "chat.not_found")?;
            Ok(object.into_value())
        }
        ChatMessageOutcome::MessageNotFound {
            index,
            total_messages,
        } => {
            let object = Object::new(ctx.clone())?;
            object.set("ok", false)?;
            object.set("reason", "chat.message_not_found")?;
            object.set("index", index)?;
            object.set("totalMessages", total_messages)?;
            Ok(object.into_value())
        }
        ChatMessageOutcome::InvalidRange { index, message } => {
            let object = Object::new(ctx.clone())?;
            object.set("ok", false)?;
            object.set("reason", "chat.invalid_message_range")?;
            object.set("index", index)?;
            object.set("message", message)?;
            Ok(object.into_value())
        }
        ChatMessageOutcome::MessageTooLarge {
            index,
            total_bytes,
            max_bytes,
        } => {
            let object = Object::new(ctx.clone())?;
            object.set("ok", false)?;
            object.set("reason", "chat.message_too_large")?;
            object.set("index", index)?;
            object.set("totalBytes", total_bytes)?;
            object.set("maxBytes", max_bytes)?;
            Ok(object.into_value())
        }
        ChatMessageOutcome::CallTooLarge {
            index,
            used_bytes,
            max_bytes,
        } => {
            let object = Object::new(ctx.clone())?;
            object.set("ok", false)?;
            object.set("reason", "chat.call_too_large")?;
            object.set("index", index)?;
            object.set("usedBytes", used_bytes)?;
            object.set("maxBytes", max_bytes)?;
            Ok(object.into_value())
        }
        ChatMessageOutcome::Found {
            total_messages,
            messages,
        } => {
            // Charge before building the objects, so the budget fails the call
            // instead of letting the heap grow past it.
            let bytes = messages
                .iter()
                .map(|message| message.text.len())
                .fold(0_usize, usize::saturating_add);
            files
                .charge_input(bytes)
                .map_err(|message| Exception::throw_message(ctx, &message))?;
            let items = messages
                .iter()
                .map(|message| message_object(ctx, message, total_messages))
                .collect::<Result<Vec<_>>>()?;
            if batch {
                let object = Object::new(ctx.clone())?;
                object.set("ok", true)?;
                object.set("totalMessages", total_messages)?;
                object.set("messages", items)?;
                Ok(object.into_value())
            } else {
                items
                    .into_iter()
                    .next()
                    .ok_or_else(|| Exception::throw_message(ctx, "chat read returned no message"))
            }
        }
    }
}

fn unsupported_result<'js>(ctx: &Ctx<'js>) -> Result<Value<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("ok", false)?;
    object.set("reason", "chat.unsupported")?;
    Ok(object.into_value())
}

fn message_object<'js>(
    ctx: &Ctx<'js>,
    message: &ChatMessageRead,
    total_messages: usize,
) -> Result<Value<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("ok", true)?;
    object.set("index", message.index)?;
    object.set("role", message.role)?;
    match &message.name {
        Some(name) => object.set("name", name.as_str())?,
        None => object.set("name", Value::new_null(ctx.clone()))?,
    }
    match &message.send_date {
        Some(date) => object.set("sendDate", date.as_str())?,
        None => object.set("sendDate", Value::new_null(ctx.clone()))?,
    }
    object.set("text", message.text.as_str())?;
    object.set("ref", message.ref_id.as_str())?;
    object.set("startLine", message.start_line)?;
    object.set("endLine", message.end_line)?;
    object.set("totalLines", message.total_lines)?;
    object.set("totalBytes", message.total_bytes)?;
    object.set("preview", message.preview)?;
    object.set("totalMessages", total_messages)?;
    Ok(object.into_value())
}
