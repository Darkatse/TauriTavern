//! The stream carries each status as the Horde wrote it; only the last carries the generation,
//! and `[DONE]` follows it.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::time::MissedTickBehavior;

use tt_ports::generation_stream::GenerationStreamSender;
use tt_ports::repositories::horde_repository::HordeRepository;

use crate::errors::ApplicationError;
use crate::services::generation_runtime::InterruptCleanup;

const STATUS_INTERVAL: Duration = Duration::from_millis(2500);
const STATUS_CHECKS: u32 = 480;

pub(super) fn task_id(task: &Value) -> Result<String, ApplicationError> {
    task.get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            let reason = task
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no task id");
            ApplicationError::InternalError(format!(
                "The Horde did not queue the generation: {reason}"
            ))
        })
}

pub(super) fn cancel_task(horde: Arc<dyn HordeRepository>, task_id: String) -> InterruptCleanup {
    Box::pin(async move {
        if let Err(error) = horde.cancel_text_generation(&task_id).await {
            tracing::warn!("Failed to cancel the Horde text generation: {error}");
        }
    })
}

pub(super) async fn follow_task(
    horde: &dyn HordeRepository,
    task_id: &str,
    sender: &GenerationStreamSender,
) -> Result<(), ApplicationError> {
    let mut checks = tokio::time::interval(STATUS_INTERVAL);
    checks.set_missed_tick_behavior(MissedTickBehavior::Delay);

    for _ in 0..STATUS_CHECKS {
        checks.tick().await;
        let status = horde.text_generation_status(task_id).await?;
        let finished = is_finished(&status)?;
        let _ = sender.send(status.to_string());
        if finished {
            let _ = sender.send("[DONE]".to_string());
            return Ok(());
        }
    }

    Err(ApplicationError::Transient(
        "Horde request timed out. Try again".to_string(),
    ))
}

fn is_finished(status: &Value) -> Result<bool, ApplicationError> {
    let flag = |key| status.get(key).and_then(Value::as_bool);
    if flag("faulted") == Some(true) {
        return Err(ApplicationError::Transient(
            "Horde request faulted. Please try again.".to_string(),
        ));
    }
    if flag("is_possible") == Some(false) {
        return Err(ApplicationError::ValidationError(
            "There are no Horde workers that are able to generate text with your request. \
             Please change the parameters or try again later."
                .to_string(),
        ));
    }
    if flag("done") != Some(true) {
        return Ok(false);
    }

    let generated = status
        .get("generations")
        .and_then(Value::as_array)
        .is_some_and(|generations| !generations.is_empty());
    if !generated {
        return Err(ApplicationError::InternalError(
            "The Horde finished the generation without a result".to_string(),
        ));
    }
    Ok(true)
}
