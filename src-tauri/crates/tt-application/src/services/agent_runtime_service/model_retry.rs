use std::time::Duration;

use serde_json::json;

use super::tool_call_projection::ToolCallProjector;
use super::{AgentCancelReceiver, AgentRuntimeService};
use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::AgentModelExchange;
use tt_domain::models::agent::profile::AgentModelRetryPolicy;
use tt_domain::models::agent::{AgentInvocation, AgentModelRequest, AgentRunEventLevel};

impl AgentRuntimeService {
    pub(super) async fn generate_model_with_retry(
        &self,
        invocation: &AgentInvocation,
        round: usize,
        request: &AgentModelRequest,
        retry: &AgentModelRetryPolicy,
        stream: bool,
        cancel: &mut AgentCancelReceiver,
    ) -> Result<AgentModelExchange, ApplicationError> {
        let run_id = invocation.run_id.as_str();
        let invocation_id = invocation.id.as_str();
        let active_run = self.active_run_handle(run_id).await?;
        let mut attempt = 1_usize;

        loop {
            self.event(
                run_id,
                AgentRunEventLevel::Debug,
                "model_call_attempt_started",
                json!({
                    "round": round,
                    "invocationId": invocation_id,
                    "attempt": attempt,
                    "maxRetries": retry.max_retries,
                }),
            )
            .await?;

            let mut projector = stream.then(|| {
                ToolCallProjector::new(
                    invocation_id,
                    invocation.exit_policy,
                    round,
                    attempt,
                    active_run.live_projection.clone(),
                )
            });
            let result = match projector.as_mut() {
                Some(projector) => {
                    let mut observe = |delta| projector.observe(delta);
                    self.model_gateway
                        .generate_with_cancel(request, Some(&mut observe), cancel.clone())
                        .await
                }
                None => {
                    self.model_gateway
                        .generate_with_cancel(request, None, cancel.clone())
                        .await
                }
            };

            match result {
                Ok(exchange) => return Ok(exchange),
                Err(error) => {
                    if let Some(projector) = projector {
                        projector.clear();
                    }
                    let retryable = error.is_retryable();
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
                            "invocationId": invocation_id,
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
                            "invocationId": invocation_id,
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
