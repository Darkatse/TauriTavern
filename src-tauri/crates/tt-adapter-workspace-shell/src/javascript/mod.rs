mod cli;
mod engine;
mod files;
mod loader;
mod runtime;
mod shell;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use bashkit::{Builtin, BuiltinContext, ExecResult};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tt_domain::errors::DomainError;
use tt_ports::workspace_shell::WorkspaceShellContext;

use cli::{Command, Script};
use files::Files;
use shell::Nesting;

type Job = JoinHandle<Result<ExecResult, DomainError>>;

pub(crate) struct Javascript {
    context: Arc<WorkspaceShellContext>,
    current: Mutex<Option<Job>>,
    /// Nested shell state of the running script, so the surrounding shell can
    /// stop it when it stops the script's interpreter.
    nesting: Arc<Nesting>,
}

impl Javascript {
    pub(crate) fn new(context: Arc<WorkspaceShellContext>) -> Self {
        Self {
            context,
            current: Mutex::new(None),
            nesting: Arc::default(),
        }
    }

    /// Stop the nested command of a running script at its next command boundary.
    pub(crate) fn cancel(&self) {
        self.nesting.cancel();
    }

    pub(crate) fn builtin(self: &Arc<Self>, name: &'static str) -> Box<dyn Builtin> {
        Box::new(JavascriptBuiltin {
            execution: self.clone(),
            name,
        })
    }

    async fn run(
        &self,
        script: Script,
        cwd: String,
        files: Files,
    ) -> Result<ExecResult, DomainError> {
        let files = files.at(&cwd);
        let mut current = self.current.lock().await;
        // A shell timeout may discard the previous waiter. Join that script before
        // starting another; the shell is sequential and needs only this one slot.
        join_current(&mut current).await?;
        let permit = crate::JAVASCRIPT_JOBS.acquire().await.map_err(|error| {
            DomainError::InternalError(format!("JavaScript execution queue closed: {error}"))
        })?;
        let context = self.context.clone();
        let nesting = self.nesting.clone();
        *current = Some(tokio::task::spawn_blocking(move || {
            let _permit = permit;
            engine::execute(script, cwd, files, context, nesting)
        }));
        Ok(join_current(&mut current)
            .await?
            .expect("the JavaScript job was registered"))
    }

    pub(crate) async fn finish(&self) -> Result<(), DomainError> {
        join_current(&mut *self.current.lock().await).await?;
        Ok(())
    }
}

async fn join_current(current: &mut Option<Job>) -> Result<Option<ExecResult>, DomainError> {
    let Some(job) = current.as_mut() else {
        return Ok(None);
    };
    // Borrow the handle: cancellation of the waiter must not detach the worker.
    let result = job.await;
    *current = None;
    result
        .map_err(|error| DomainError::InternalError(format!("JavaScript worker failed: {error}")))?
        .map(Some)
}

struct JavascriptBuiltin {
    execution: Arc<Javascript>,
    name: &'static str,
}

#[async_trait]
impl Builtin for JavascriptBuiltin {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let command = match cli::parse(self.name, ctx.args, ctx.stdin_bytes()) {
            Ok(command) => command,
            Err(message) => return Ok(ExecResult::err(format!("js: {message}\n"), 2)),
        };
        let Command::Run(script) = command else {
            return Ok(ExecResult::ok(cli::HELP));
        };
        let budget = ctx.execution_budget().ok_or_else(|| {
            std::io::Error::other(DomainError::InternalError(
                "JavaScript command has no shell execution budget".into(),
            ))
        })?;
        self.execution
            .run(
                script,
                ctx.cwd.to_string_lossy().into_owned(),
                Files::new(
                    ctx.fs.clone(),
                    tokio::runtime::Handle::current(),
                    budget,
                    self.execution.context.clone(),
                ),
            )
            .await
            .map_err(|error| std::io::Error::other(error).into())
    }
}
