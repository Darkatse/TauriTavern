use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use bashkit::ExecResult;
use rquickjs::{Coerced, Context, Ctx, FromJs, Module, Runtime, Value};
use tt_domain::errors::DomainError;
use tt_ports::workspace_shell::WorkspaceShellContext;

use super::cli::{Script, Source};
use super::files::{Files, resolve_path};
use super::loader::{ModuleLoader, ModuleResolver, ensure_module_path};
use super::runtime::{
    MAX_OUTPUT_BYTES, Output, RUNTIME_MODULE, RuntimeModule, RuntimeState, output_object,
    process_object,
};

pub(super) fn execute(
    script: Script,
    cwd: String,
    files: Files,
    host: Arc<WorkspaceShellContext>,
) -> Result<ExecResult, DomainError> {
    let (name, source, entry) = match source(script.source, &cwd, &files) {
        Ok(source) => source,
        Err(message) => return Ok(ExecResult::err(format!("js: {message}\n"), 1)),
    };
    let mut argv = vec![script.command];
    argv.extend(entry);
    argv.extend(script.args);
    let runtime = Runtime::new().map_err(internal_error)?;
    runtime.set_memory_limit(32 * 1024 * 1024);
    runtime.set_max_stack_size(256 * 1024);
    let control = files.clone();
    runtime.set_interrupt_handler(Some(Box::new(move || control.check().is_err())));
    runtime.set_loader(ModuleResolver, ModuleLoader(files.clone()));
    let context = Context::full(&runtime).map_err(internal_error)?;
    let output = Rc::new(RefCell::new(Output::default()));
    let exit_code = Rc::new(Cell::new(0));
    let outcome = context.with(|ctx| {
        ctx.store_userdata(RuntimeState {
            files: files.clone(),
            context: host,
            output: output.clone(),
        })
        .map_err(|_| rquickjs::Error::Unknown)?;
        ctx.globals()
            .set("console", output_object(&ctx, output.clone(), false)?)?;
        ctx.globals()
            .set("process", process_object(&ctx, argv, exit_code.clone())?)?;
        Module::declare_def::<RuntimeModule, _>(ctx.clone(), RUNTIME_MODULE)?;
        let (_, evaluated) = Module::declare(ctx.clone(), name.clone(), source)?.eval()?;
        evaluated.finish::<Value>()?;
        Ok(())
    });
    let failure = outcome.err().map(|error| match files.check() {
        Err(message) => message,
        Ok(()) => context.with(|ctx| exception_message(&ctx, &error)),
    });
    let mut output = output.borrow_mut();
    let exit_code = if let Some(message) = failure {
        let message = format!("js: {name}: {message}\n");
        // Keep the cause even when earlier diagnostics filled stderr.
        let message = &message[..message.floor_char_boundary(MAX_OUTPUT_BYTES)];
        let remaining = MAX_OUTPUT_BYTES - message.len();
        let keep = output.stderr.floor_char_boundary(remaining);
        output.stderr.truncate(keep);
        output.stderr.push_str(message);
        1
    } else {
        i32::from(exit_code.get())
    };
    Ok(ExecResult {
        stdout: std::mem::take(&mut output.stdout).into(),
        stderr: std::mem::take(&mut output.stderr).into(),
        exit_code,
        ..Default::default()
    })
}

fn source(
    source: Source,
    cwd: &str,
    files: &Files,
) -> Result<(String, String, Option<String>), String> {
    match source {
        Source::Eval(code) => Ok((resolve_path(cwd, "<eval>")?, code, None)),
        Source::Stdin(code) => Ok((resolve_path(cwd, "<stdin>")?, code, Some("-".into()))),
        Source::File(raw) => {
            let path = resolve_path(cwd, &raw)?;
            ensure_module_path(&path)?;
            let text = files.read(&path)?;
            Ok((path.clone(), text, Some(path)))
        }
    }
}

fn internal_error(error: rquickjs::Error) -> DomainError {
    DomainError::InternalError(format!("Cannot initialize JavaScript runtime: {error}"))
}

fn exception_message(ctx: &Ctx<'_>, error: &rquickjs::Error) -> String {
    if matches!(error, rquickjs::Error::WouldBlock) {
        return "The awaited Promise is still pending, but no work remains to resolve it.".into();
    }
    if !matches!(error, rquickjs::Error::Exception) {
        return error.to_string();
    }
    let thrown = ctx.catch();
    let message = Coerced::<String>::from_js(ctx, thrown.clone())
        .map(|value| value.0)
        .unwrap_or_else(|_| "JavaScript threw a value that cannot be rendered.".into());
    let stack = thrown
        .as_object()
        .and_then(|object| object.get::<_, String>("stack").ok());
    match stack {
        Some(stack) => format!("{message}\n{stack}"),
        None => message,
    }
}
