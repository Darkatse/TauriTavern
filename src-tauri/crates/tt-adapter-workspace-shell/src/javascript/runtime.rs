use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use rquickjs::function::{Opt, Rest};
use rquickjs::module::{Declarations, Exports, ModuleDef};
use rquickjs::object::Accessor;
use rquickjs::{Coerced, Ctx, Exception, FromJs, Function, JsLifetime, Object, Result, Value};
use tt_domain::frozen_macros::MAX_EXPANDED_TEXT_BYTES;
use tt_ports::workspace_shell::WorkspaceShellContext;

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
    pub files: Files,
    pub context: Arc<WorkspaceShellContext>,
    pub output: Rc<RefCell<Output>>,
}

// Host-owned data only; no QuickJS references whose lifetime needs changing.
unsafe impl<'js> JsLifetime<'js> for RuntimeState {
    type Changed<'to> = Self;
}

pub(super) struct RuntimeModule;

impl ModuleDef for RuntimeModule {
    fn declare(exports: &Declarations<'_>) -> Result<()> {
        for name in ["workspace", "context", "macros", "log"] {
            exports.declare(name)?;
        }
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        let (files, context, output) = {
            let state = ctx.userdata::<RuntimeState>().ok_or_else(|| {
                Exception::throw_message(ctx, "JavaScript runtime context is missing")
            })?;
            (
                state.files.clone(),
                state.context.clone(),
                state.output.clone(),
            )
        };
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
        exports.export("log", output_object(ctx, output, true)?)?;
        Ok(())
    }
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
