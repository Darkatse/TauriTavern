use std::time::Duration;

use serde_json::json;

use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::AgentModelExchange;
use crate::domain::models::agent::profile::AgentModelRetryPolicy;
use crate::domain::models::agent::{AgentModelRequest, AgentRunEventLevel};

impl AgentRuntimeService {
    pub(super) async fn generate_model_with_retry(
        &self,
        run_id: &str,
        round: usize,
        request: &AgentModelRequest,
        retry: &AgentModelRetryPolicy,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<AgentModelExchange, ApplicationError> {
        let mut attempt = 1_usize;

        loop {
            self.event(
                run_id,
                AgentRunEventLevel::Debug,
                "model_call_attempt_started",
                json!({
                    "round": round,
                    "attempt": attempt,
                    "maxRetries": retry.max_retries,
                }),
            )
            .await?;

            match self
                .model_gateway
                .generate_with_cancel(request.clone(), cancel.clone())
                .await
            {
                Ok(exchange) => return Ok(exchange),
                Err(error) => {
                    let retryable = is_retryable_model_error(&error);
                    let will_retry = retryable && attempt <= retry.max_retries;
                    self.event(
                        run_id,
                        if will_retry {
                            AgentRunEventLevel::Warn
                        } else {
                            AgentRunEventLevel::Error
                        },
                        "model_call_attempt_failed",
                        json!({
                            "round": round,
                            "attempt": attempt,
                            "maxRetries": retry.max_retries,
                            "retryable": retryable,
                            "willRetry": will_retry,
                            "message": error.to_string(),
                        }),
                    )
                    .await?;

                    if !will_retry {
                        return Err(error);
                    }

                    self.event(
                        run_id,
                        AgentRunEventLevel::Warn,
                        "model_call_retry_scheduled",
                        json!({
                            "round": round,
                            "nextAttempt": attempt + 1,
                            "intervalMs": retry.interval_ms,
                        }),
                    )
                    .await?;
                    self.sleep_or_cancel(Duration::from_millis(retry.interval_ms), cancel)
                        .await?;
                    attempt += 1;
                }
            }
        }
    }

    async fn sleep_or_cancel(
        &self,
        duration: Duration,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<(), ApplicationError> {
        if duration.is_zero() {
            return self.ensure_not_cancelled(cancel);
        }

        let sleep = tokio::time::sleep(duration);
        tokio::pin!(sleep);

        loop {
            tokio::select! {
                _ = &mut sleep => return Ok(()),
                changed = cancel.changed() => {
                    if changed.is_err() {
                        return Ok(());
                    }
                    if *cancel.borrow() {
                        return self.ensure_not_cancelled(cancel);
                    }
                }
            }
        }
    }
}

fn is_retryable_model_error(error: &ApplicationError) -> bool {
    matches!(
        error,
        ApplicationError::RateLimited(_) | ApplicationError::Transient(_)
    )
}
