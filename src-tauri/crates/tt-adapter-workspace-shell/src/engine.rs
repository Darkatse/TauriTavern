use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use async_trait::async_trait;
use bashkit::{Bash, ExecutionLimits, FileSystem, PythonLimits};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tt_domain::errors::DomainError;
use tt_ports::workspace_shell::{
    WorkspaceShell, WorkspaceShellExit, WorkspaceShellRequest, WorkspaceShellResult,
};

use crate::filesystem::WorkspaceFileSystem;
use crate::javascript::Javascript;

const EXECUTION_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const MAX_COMMAND_BYTES: usize = 128 * 1024;
pub(crate) const MAX_OUTPUT_BYTES: usize = 128 * 1024;

pub struct WorkspaceShellEngine;

#[async_trait]
impl WorkspaceShell for WorkspaceShellEngine {
    async fn execute(
        &self,
        request: WorkspaceShellRequest,
    ) -> Result<WorkspaceShellResult, DomainError> {
        let WorkspaceShellRequest {
            command,
            workdir,
            files,
            context,
            mut cancel,
        } = request;
        if *cancel.borrow() {
            return Ok(stopped(WorkspaceShellExit::Cancelled));
        }
        let files = Arc::new(WorkspaceFileSystem::new(files));
        let javascript = Arc::new(Javascript::new(context));
        let workdir = bashkit::normalize_path(Path::new(&workdir));
        let mut bash = Bash::builder()
            .fs(files.clone())
            .builtin("js", javascript.builtin("js"))
            .builtin("node", javascript.builtin("node"))
            .builtin("deno", javascript.builtin("deno"))
            .cwd(workdir.clone())
            .env("HOME", "/")
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .python_with_limits(PythonLimits::default().max_duration(EXECUTION_TIMEOUT))
            .limits(
                ExecutionLimits::new()
                    .timeout(EXECUTION_TIMEOUT)
                    .max_input_bytes(MAX_COMMAND_BYTES)
                    .max_stdout_bytes(MAX_OUTPUT_BYTES)
                    .max_stderr_bytes(MAX_OUTPUT_BYTES),
            )
            .build();
        let cancellation = bash.cancellation_token();
        let mut worker_cancel = cancel.clone();
        let runtime = Handle::current();
        let mut worker = tokio::task::spawn_blocking(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                runtime.block_on(async {
                    tokio::select! {
                        biased;
                        _ = cancelled(&mut worker_cancel) => Err(bashkit::Error::Cancelled),
                        result = async {
                            if !files.stat(&workdir).await?.file_type.is_dir() {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::NotADirectory,
                                    "workdir is not a directory",
                                ).into());
                            }
                            bash.exec(&command).await
                        } => result,
                    }
                })
            }));
            // This runs even when polling the interpreter panics or its internal
            // timeout drops an awaited write.
            let js_finished = runtime.block_on(javascript.finish());
            let files_finished = runtime.block_on(files.finish());
            js_finished?;
            files_finished?;
            let result = result.map_err(|_| {
                DomainError::InternalError("Workspace shell worker panicked".into())
            })?;
            map_result(result)
        });

        let result = tokio::select! {
            result = &mut worker => result,
            _ = cancelled(&mut cancel) => {
                // This observer runs outside the blocking worker so CPU-bound
                // interpreter polls can see cancellation at their checkpoints.
                cancellation.store(true, Ordering::Relaxed);
                worker.await
            }
        };
        result.map_err(|error| {
            DomainError::InternalError(format!("Workspace shell worker failed: {error}"))
        })?
    }
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

fn stopped(exit: WorkspaceShellExit) -> WorkspaceShellResult {
    WorkspaceShellResult {
        stdout: String::new(),
        stderr: String::new(),
        exit,
        output_truncated: false,
    }
}

fn map_result(
    result: bashkit::Result<bashkit::ExecResult>,
) -> Result<WorkspaceShellResult, DomainError> {
    use bashkit::{Error, ExecutionBudgetExceeded, LimitExceeded};

    if let Err(Error::Io(error)) = &result
        && let Some(DomainError::InternalError(message)) = error
            .get_ref()
            .and_then(|source| source.downcast_ref::<DomainError>())
    {
        return Err(DomainError::InternalError(message.clone()));
    }
    Ok(match result {
        Ok(result) => WorkspaceShellResult {
            stdout: result.stdout.text_lossy().into_owned(),
            stderr: result.stderr.text_lossy().into_owned(),
            exit: WorkspaceShellExit::Exited(result.exit_code),
            output_truncated: result.stdout_truncated || result.stderr_truncated,
        },
        Err(
            Error::Cancelled
            | Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::Cancelled,
            )),
        ) => stopped(WorkspaceShellExit::Cancelled),
        Err(Error::ResourceLimit(
            LimitExceeded::Timeout(_)
            | LimitExceeded::ParserTimeout(_)
            | LimitExceeded::ExecutionBudget(ExecutionBudgetExceeded::Deadline { .. }),
        )) => stopped(WorkspaceShellExit::TimedOut),
        Err(error) => WorkspaceShellResult {
            stderr: error.to_string(),
            ..stopped(WorkspaceShellExit::Failed)
        },
    })
}
