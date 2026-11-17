use std::future::Future;
use std::sync::Arc;

use tokio::sync::watch;
use tt_domain::errors::DomainError;
use tt_ports::generation_background::{GenerationBackgroundOutcome, GenerationBackgroundRuntime};
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};

use crate::dto::generation_stream_dto::GenerationStreamReadResultDto;
use crate::errors::ApplicationError;
use crate::services::request_cancellation::CancellationRegistry;

mod background;
mod stream_session;

use self::background::GenerationBackgroundLease;
use self::stream_session::{StreamAppendOutcome, StreamSessionRegistry};

/// Generations started by the page, for every API family. A request is cancelled by its ID; a
/// stream keeps its events until the page acknowledges them, so a suspended WebView can read them
/// again. Both hold a background lease while they run.
pub struct GenerationRuntime {
    background: Option<Arc<dyn GenerationBackgroundRuntime>>,
    streams: StreamSessionRegistry,
    requests: CancellationRegistry,
}

impl GenerationRuntime {
    pub fn new(background: Option<Arc<dyn GenerationBackgroundRuntime>>) -> Self {
        Self {
            background,
            streams: StreamSessionRegistry::default(),
            requests: CancellationRegistry::default(),
        }
    }

    pub async fn run_request<T>(
        &self,
        request_id: &str,
        notify_completion: bool,
        generation: impl Future<Output = Result<T, ApplicationError>>,
    ) -> Result<T, ApplicationError> {
        let request = self.requests.register(request_id)?;
        let background = GenerationBackgroundLease::start(
            self.background.as_ref(),
            request_id,
            notify_completion,
        );
        let result = until_cancelled(generation, request.receiver()).await;
        drop(request);
        background.complete(background_outcome(&result), notify_completion);
        result
    }

    pub fn cancel_request(&self, request_id: &str) -> bool {
        self.requests.cancel(request_id)
    }

    /// `generate` sends the upstream stream events and returns when the upstream stream ends.
    pub async fn start_stream<F, Fut>(
        self: &Arc<Self>,
        stream_id: String,
        notify_completion: bool,
        generate: F,
    ) -> Result<(), ApplicationError>
    where
        F: FnOnce(GenerationStreamSender, GenerationCancelReceiver) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), ApplicationError>> + Send + 'static,
    {
        let cancel = self.streams.register(&stream_id).await?;
        let background = GenerationBackgroundLease::start(
            self.background.as_ref(),
            &stream_id,
            notify_completion,
        );
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime
                .run_stream(stream_id, generate, cancel, background, notify_completion)
                .await;
        });
        Ok(())
    }

    pub async fn read_stream(
        &self,
        stream_id: &str,
        after_seq: u64,
    ) -> Result<GenerationStreamReadResultDto, ApplicationError> {
        self.streams.read(stream_id, after_seq).await
    }

    pub async fn close_stream(&self, stream_id: &str) -> bool {
        self.streams.close(stream_id).await
    }

    async fn run_stream<F, Fut>(
        self: Arc<Self>,
        stream_id: String,
        generate: F,
        mut lifecycle_cancel: watch::Receiver<bool>,
        mut background: GenerationBackgroundLease,
        notify_completion: bool,
    ) where
        F: FnOnce(GenerationStreamSender, GenerationCancelReceiver) -> Fut,
        Fut: Future<Output = Result<(), ApplicationError>> + Send + 'static,
    {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (stop_provider, provider_cancel) = watch::channel(false);
        let generation_task = tokio::spawn(generate(sender, provider_cancel));
        let mut received_bytes = 0_u64;

        let result = loop {
            let chunk = tokio::select! {
                chunk = receiver.recv() => chunk,
                Ok(_) = lifecycle_cancel.wait_for(|cancelled| *cancelled) => {
                    generation_task.abort();
                    break Err(DomainError::generation_cancelled_by_user().into());
                }
            };
            let Some(chunk) = chunk else {
                break match generation_task.await {
                    Ok(result) => result,
                    Err(error) => Err(ApplicationError::InternalError(format!(
                        "Streaming task join failed: {error}"
                    ))),
                };
            };

            if chunk.is_empty() {
                continue;
            }

            received_bytes = received_bytes.saturating_add(chunk.len() as u64);
            background.report_progress(received_bytes);

            match self.streams.append(&stream_id, chunk).await {
                Ok(StreamAppendOutcome::Appended) => {}
                Ok(StreamAppendOutcome::ProviderDone) => {
                    finish_provider_after_done(&stream_id, stop_provider, generation_task).await;
                    break Ok(());
                }
                Ok(StreamAppendOutcome::SessionClosed) => {
                    generation_task.abort();
                    break Err(DomainError::generation_cancelled_by_user().into());
                }
                Err(error) => {
                    generation_task.abort();
                    break Err(error);
                }
            }
        };

        let outcome = background_outcome(&result);
        match result {
            Ok(()) => self.streams.finish(&stream_id).await,
            Err(error) => self.streams.fail(&stream_id, error).await,
        }
        background.complete(outcome, notify_completion);
    }
}

/// Returns a user cancellation as soon as `cancel` turns `true`, dropping the generation.
pub(crate) async fn until_cancelled<T>(
    generation: impl Future<Output = Result<T, ApplicationError>>,
    mut cancel: GenerationCancelReceiver,
) -> Result<T, ApplicationError> {
    tokio::select! {
        result = generation => result,
        Ok(_) = cancel.wait_for(|cancelled| *cancelled) => {
            Err(DomainError::generation_cancelled_by_user().into())
        }
    }
}

async fn finish_provider_after_done(
    stream_id: &str,
    stop_provider: watch::Sender<bool>,
    generation_task: tokio::task::JoinHandle<Result<(), ApplicationError>>,
) {
    let _ = stop_provider.send(true);
    match generation_task.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!(
            stream_id,
            "Provider stream cleanup failed after completion: {error}"
        ),
        Err(error) => tracing::error!(
            stream_id,
            "Provider stream cleanup task failed after completion: {error}"
        ),
    }
}

fn background_outcome<T>(result: &Result<T, ApplicationError>) -> GenerationBackgroundOutcome {
    match result {
        Ok(_) => GenerationBackgroundOutcome::Succeeded,
        Err(ApplicationError::Cancelled(_)) => GenerationBackgroundOutcome::Cancelled,
        Err(ApplicationError::RateLimited(_)) => GenerationBackgroundOutcome::Failed {
            status_code: Some(429),
        },
        Err(_) => GenerationBackgroundOutcome::Failed { status_code: None },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[tokio::test]
    async fn provider_done_waits_for_provider_cleanup() {
        let (stop_provider, mut provider_cancel) = watch::channel(false);
        let cleaned_up = Arc::new(AtomicBool::new(false));
        let generation_task = tokio::spawn({
            let cleaned_up = cleaned_up.clone();
            async move {
                provider_cancel.changed().await.unwrap();
                cleaned_up.store(true, Ordering::SeqCst);
                Ok::<(), ApplicationError>(())
            }
        });

        finish_provider_after_done("stream-1", stop_provider, generation_task).await;

        assert!(cleaned_up.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn until_cancelled_observes_already_cancelled_receiver_and_ignores_dropped_sender() {
        let (sender, mut initial) = watch::channel(false);
        sender.send(true).unwrap();
        let _ = initial.borrow_and_update();
        let cloned = initial.clone();

        let cancelled = until_cancelled(
            std::future::pending::<Result<(), ApplicationError>>(),
            cloned,
        )
        .await;
        assert!(matches!(cancelled, Err(ApplicationError::Cancelled(_))));

        let (sender, cancel) = watch::channel(false);
        drop(sender);
        let completed = until_cancelled(
            async {
                tokio::task::yield_now().await;
                Ok::<_, ApplicationError>(42)
            },
            cancel,
        )
        .await;
        assert_eq!(completed.unwrap(), 42);
    }

    #[tokio::test]
    async fn run_request_rejects_duplicate_id_and_releases_when_dropped() {
        let runtime = Arc::new(GenerationRuntime::new(None));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();

        let first = tokio::spawn({
            let runtime = runtime.clone();
            async move {
                runtime
                    .run_request("req-1", false, async move {
                        let _ = started_tx.send(());
                        release_rx.await.unwrap();
                        Ok::<_, ApplicationError>("ok")
                    })
                    .await
            }
        });
        started_rx.await.unwrap();

        let duplicate = runtime
            .run_request("req-1", false, async { Ok::<_, ApplicationError>("dup") })
            .await;
        assert!(matches!(duplicate, Err(ApplicationError::Conflict(_))));

        release_tx.send(()).unwrap();
        assert_eq!(first.await.unwrap().unwrap(), "ok");

        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        {
            let dropped_request = runtime.run_request("req-drop", false, async move {
                let _ = entered_tx.send(());
                std::future::pending::<Result<(), ApplicationError>>().await
            });
            tokio::select! {
                _ = dropped_request => panic!("pending generation should not resolve"),
                _ = entered_rx => {}
            }
        }

        let reused = runtime
            .run_request("req-drop", false, async {
                Ok::<_, ApplicationError>("reused")
            })
            .await
            .unwrap();
        assert_eq!(reused, "reused");
    }
}
