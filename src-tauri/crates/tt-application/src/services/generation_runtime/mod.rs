use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::watch;
use tt_domain::errors::DomainError;
use tt_ports::generation_background::{GenerationBackgroundOutcome, GenerationBackgroundRuntime};
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};

use crate::dto::generation_stream_dto::GenerationStreamReadResultDto;
use crate::errors::ApplicationError;
use crate::services::request_cancellation::CancellationRegistry;
pub use crate::services::request_cancellation::RegisteredRequest;

mod background;
mod stream_session;

use self::background::GenerationBackgroundLease;
use self::stream_session::{StreamAppendOutcome, StreamSessionRegistry};

/// Runs once after the page cancels a generation, before the generation reports its end. A backend
/// that keeps generating after its connection closes is told to stop here.
pub type InterruptCleanup = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Generations started by the page, for every API family. A request is cancelled by its ID from
/// the moment its command registers it; a stream keeps its events until the page acknowledges
/// them, so a suspended WebView can read them again. Both hold a background lease while they run.
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

    /// Register a request before the command waits on anything, such as endpoint consent.
    pub fn register_request(
        &self,
        request_id: &str,
    ) -> Result<RegisteredRequest<'_>, ApplicationError> {
        self.requests.register(request_id)
    }

    /// A request cancelled before it runs never starts its generation. `on_interrupt` runs only when
    /// an already-started generation is cancelled explicitly via `cancel_request`.
    pub async fn run_request<T>(
        &self,
        request: RegisteredRequest<'_>,
        notify_completion: bool,
        on_interrupt: Option<InterruptCleanup>,
        generation: impl Future<Output = Result<T, ApplicationError>>,
    ) -> Result<T, ApplicationError> {
        if request.is_cancelled() {
            return Err(DomainError::generation_cancelled_by_user().into());
        }

        let background = GenerationBackgroundLease::start(
            self.background.as_ref(),
            request.id(),
            notify_completion,
        );
        let result = until_cancelled(generation, request.receiver()).await;
        drop(request);
        clean_up_if_interrupted(&result, on_interrupt).await;
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
        on_interrupt: Option<InterruptCleanup>,
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
                .run_stream(
                    stream_id,
                    generate,
                    cancel,
                    background,
                    notify_completion,
                    on_interrupt,
                )
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
        on_interrupt: Option<InterruptCleanup>,
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

        clean_up_if_interrupted(&result, on_interrupt).await;
        let outcome = background_outcome(&result);
        match result {
            Ok(()) => self.streams.finish(&stream_id).await,
            Err(error) => self.streams.fail(&stream_id, error).await,
        }
        background.complete(outcome, notify_completion);
    }
}

/// A `quiet` generation, such as a summary, runs for the page itself; its end is not notified.
pub(crate) fn notifies_completion(payload: &Map<String, Value>) -> bool {
    !payload
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("quiet"))
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

async fn clean_up_if_interrupted<T>(
    result: &Result<T, ApplicationError>,
    on_interrupt: Option<InterruptCleanup>,
) {
    if let (Err(ApplicationError::Cancelled(_)), Some(cleanup)) = (result, on_interrupt) {
        cleanup.await;
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
    async fn a_request_id_is_taken_until_its_run_ends_or_is_dropped() {
        let runtime = GenerationRuntime::new(None);
        let request = runtime.register_request("req").unwrap();
        assert!(matches!(
            runtime.register_request("req"),
            Err(ApplicationError::Conflict(_))
        ));

        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        {
            let run = runtime.run_request(request, false, None, async move {
                let _ = entered_tx.send(());
                std::future::pending::<Result<(), ApplicationError>>().await
            });
            tokio::select! {
                _ = run => panic!("pending generation should not resolve"),
                _ = entered_rx => {}
            }
        }

        let request = runtime.register_request("req").unwrap();
        runtime
            .run_request(request, false, None, async {
                Ok::<_, ApplicationError>(())
            })
            .await
            .unwrap();
        assert!(runtime.register_request("req").is_ok());
    }

    #[tokio::test]
    async fn interrupt_cleanup_runs_once_only_for_a_started_generation_that_is_cancelled() {
        let runtime = Arc::new(GenerationRuntime::new(None));
        let cleanups = Arc::new(AtomicUsize::new(0));
        let cleanup = || -> Option<InterruptCleanup> {
            let cleanups = cleanups.clone();
            Some(Box::pin(async move {
                cleanups.fetch_add(1, Ordering::SeqCst);
            }))
        };

        let completed = runtime.register_request("completed").unwrap();
        runtime
            .run_request(completed, false, cleanup(), async {
                Ok::<_, ApplicationError>(())
            })
            .await
            .unwrap();
        assert_eq!(cleanups.load(Ordering::SeqCst), 0);

        let started = AtomicBool::new(false);
        let waiting = runtime.register_request("cancelled-before-run").unwrap();
        assert!(runtime.cancel_request("cancelled-before-run"));
        let result = runtime
            .run_request(waiting, false, cleanup(), async {
                started.store(true, Ordering::SeqCst);
                Ok::<_, ApplicationError>(())
            })
            .await;
        assert!(matches!(result, Err(ApplicationError::Cancelled(_))));
        assert!(!started.load(Ordering::SeqCst));
        assert_eq!(cleanups.load(Ordering::SeqCst), 0);

        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let running = runtime.register_request("cancelled").unwrap();
        let generation = runtime.run_request(running, false, cleanup(), async move {
            let _ = entered_tx.send(());
            std::future::pending::<Result<(), ApplicationError>>().await
        });
        let cancel = async {
            entered_rx.await.unwrap();
            assert!(runtime.cancel_request("cancelled"));
        };
        let (result, ()) = tokio::join!(generation, cancel);

        assert!(matches!(result, Err(ApplicationError::Cancelled(_))));
        assert_eq!(cleanups.load(Ordering::SeqCst), 1);

        let (provider_done_tx, provider_done_rx) = tokio::sync::oneshot::channel();
        runtime
            .start_stream(
                "stream-done".to_string(),
                false,
                cleanup(),
                |sender, mut cancel| async move {
                    let _ = sender.send("[DONE]".to_string());
                    let _ = cancel.changed().await;
                    let _ = provider_done_tx.send(());
                    Ok(())
                },
            )
            .await
            .unwrap();
        let _ = runtime.read_stream("stream-done", 0).await.unwrap();
        provider_done_rx.await.unwrap();
        assert!(runtime.close_stream("stream-done").await);
        tokio::task::yield_now().await;
        assert_eq!(cleanups.load(Ordering::SeqCst), 1);

        let (stream_started_tx, stream_started_rx) = tokio::sync::oneshot::channel();
        let (interrupt_done_tx, interrupt_done_rx) = tokio::sync::oneshot::channel();
        runtime
            .start_stream(
                "stream-cancelled".to_string(),
                false,
                Some(Box::pin({
                    let cleanups = cleanups.clone();
                    async move {
                        cleanups.fetch_add(1, Ordering::SeqCst);
                        let _ = interrupt_done_tx.send(());
                    }
                })),
                |sender, _cancel| async move {
                    let _sender = sender;
                    let _ = stream_started_tx.send(());
                    std::future::pending::<Result<(), ApplicationError>>().await
                },
            )
            .await
            .unwrap();
        stream_started_rx.await.unwrap();
        assert!(runtime.close_stream("stream-cancelled").await);
        interrupt_done_rx.await.unwrap();
        assert_eq!(cleanups.load(Ordering::SeqCst), 2);
    }
}
