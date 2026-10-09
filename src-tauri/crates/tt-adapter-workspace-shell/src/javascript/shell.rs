//! The `shell` namespace: run workspace commands from JavaScript.
//!
//! QuickJS ships no process primitive that could reach the scoped workspace
//! filesystem, so scripts run commands through the host. Each call builds a
//! fresh interpreter, matching the "every shell is a new environment" rule of
//! `workspace.shell`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bashkit::{Bash, ExecOptions, ExecutionLimits, PythonLimits};
use rquickjs::function::Opt;
use rquickjs::{Ctx, Exception, Function, Object, Result, Value};

use super::files::{Files, resolve_path};
use crate::engine::{
    EXECUTION_TIMEOUT, MAX_COMMAND_BYTES, MAX_OUTPUT_BYTES, require_directory, with_commands,
};

/// Handles the running `shell.exec` call exposes to the cancellation path.
///
/// A nested instance has its own cancellation token and does not observe the
/// outer one, so the token is published here for the outer observer to set.
#[derive(Default)]
pub(super) struct Nesting {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl Nesting {
    fn begin(&self, token: Arc<AtomicBool>) {
        *self.lock() = Some(token);
    }

    fn end(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Arc<AtomicBool>>> {
        // The critical section swaps a pointer and never runs user code.
        self.cancel
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Stop the nested command at its next command boundary.
    pub(super) fn cancel(&self) {
        if let Some(token) = self.lock().as_ref() {
            token.store(true, Ordering::Relaxed);
        }
    }
}

/// Build the `shell` object exported from the runtime module.
pub(super) fn shell_object<'js>(
    ctx: &Ctx<'js>,
    files: Files,
    nesting: Arc<Nesting>,
) -> Result<Object<'js>> {
    let shell = Object::new(ctx.clone())?;
    shell.set(
        "exec",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, command: String, options: Opt<Value<'js>>| {
                let options = Options::parse(options.0)
                    .map_err(|message| Exception::throw_message(&ctx, &message))?;
                run(&ctx, &files, &nesting, command, options)
                    .map_err(|message| Exception::throw_message(&ctx, &message))
            },
        )?,
    )?;
    Ok(shell)
}

/// Options accepted by `shell.exec`.
#[derive(Default)]
struct Options {
    stdin: Option<String>,
    cwd: Option<String>,
    env: Vec<(String, String)>,
}

impl Options {
    fn parse(options: Option<Value<'_>>) -> std::result::Result<Self, String> {
        let parsed = match options {
            Some(value) => value,
            None => return Ok(Self::default()),
        };
        if parsed.is_undefined() || parsed.is_null() {
            return Ok(Self::default());
        }
        let object = parsed
            .as_object()
            .ok_or("shell.exec options must be an object.")?
            .clone();
        Ok(Self {
            stdin: text(&object, "stdin")?,
            cwd: text(&object, "cwd")?,
            env: environment(&object)?,
        })
    }
}

fn text(object: &Object<'_>, name: &str) -> std::result::Result<Option<String>, String> {
    object
        .get::<_, Option<String>>(name)
        .map_err(|_| format!("shell.exec option `{name}` must be a string."))
}

fn environment(object: &Object<'_>) -> std::result::Result<Vec<(String, String)>, String> {
    let Some(value) = object
        .get::<_, Option<Value>>("env")
        .map_err(|_| "shell.exec option `env` must be an object.".to_string())?
    else {
        return Ok(Vec::new());
    };
    let env = value
        .as_object()
        .ok_or("shell.exec option `env` must be an object.")?;
    env.props::<String, String>()
        .map(|entry| {
            entry.map_err(|_| "shell.exec option `env` must map strings to strings.".to_string())
        })
        .collect()
}

fn run<'js>(
    ctx: &Ctx<'js>,
    files: &Files,
    nesting: &Nesting,
    command: String,
    options: Options,
) -> std::result::Result<Object<'js>, String> {
    files.check()?;
    // An omitted `cwd` starts where the surrounding shell started, so
    // `shell.exec('ls')` observes the same directory as a plain `ls`.
    let cwd = match &options.cwd {
        Some(path) => resolve_path(files.origin(), path)?,
        None => files.origin().to_owned(),
    };
    let mut shell = with_commands(
        Bash::builder()
            .fs(files.fs.clone())
            .env("HOME", "/")
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .python_with_limits(PythonLimits::default().max_duration(EXECUTION_TIMEOUT))
            .limits(
                ExecutionLimits::new()
                    .timeout(EXECUTION_TIMEOUT)
                    .max_input_bytes(MAX_COMMAND_BYTES)
                    .max_stdout_bytes(MAX_OUTPUT_BYTES)
                    .max_stderr_bytes(MAX_OUTPUT_BYTES),
            ),
        files.context(),
    );
    // Appended, never replacing: the base environment keeps `HOME` and the
    // in-process Python opt-in.
    for (name, value) in options.env {
        shell = shell.env(name, value);
    }
    let mut shell = shell.cwd(cwd.clone()).build();
    nesting.begin(shell.cancellation_token());
    let mut exec = ExecOptions::new();
    if let Some(stdin) = options.stdin {
        exec = exec.stdin(stdin);
    }
    let outcome = files.runtime.block_on(async {
        require_directory(files.fs.as_ref(), Path::new(&cwd)).await?;
        shell.exec_with_options(&command, exec).await
    });
    nesting.end();
    let result = outcome.map_err(|error| error.to_string())?;
    let object = Object::new(ctx.clone()).map_err(|error| error.to_string())?;
    object
        .set("stdout", result.stdout.text_lossy().into_owned())
        .and_then(|()| object.set("stderr", result.stderr.text_lossy().into_owned()))
        .and_then(|()| object.set("code", result.exit_code))
        .map_err(|error| error.to_string())?;
    Ok(object)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::Nesting;

    /// The outer cancellation path must be able to stop a nested command.
    ///
    /// A nested instance owns its own token and does not observe the outer one
    /// or the revoked filesystem scope, so the token is published for the
    /// engine's cancellation observer to set.
    #[test]
    fn cancel_stops_the_published_nested_token() {
        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let nesting = Nesting::default();
        nesting.begin(token.clone());
        assert!(!token.load(Ordering::Relaxed));
        nesting.cancel();
        assert!(
            token.load(Ordering::Relaxed),
            "cancel must set the running command's token",
        );
    }

    /// A finished call must not leave a token behind for the next one, or a
    /// later cancellation would stop an unrelated command.
    #[test]
    fn a_finished_call_leaves_no_token_behind() {
        let nesting = Nesting::default();
        nesting.begin(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )));
        nesting.end();

        let later = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        nesting.begin(later.clone());
        nesting.cancel();
        assert!(later.load(Ordering::Relaxed));
    }
}
