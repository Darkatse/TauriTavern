use std::sync::Arc;

use tt_ports::generation_background::{GenerationBackgroundOutcome, GenerationBackgroundRuntime};

const PROGRESS_REPORT_STEP_BYTES: u64 = 4 * 1024;

pub(super) struct GenerationBackgroundLease {
    runtime: Option<Arc<dyn GenerationBackgroundRuntime>>,
    task_id: String,
    last_reported_units: u64,
}

impl GenerationBackgroundLease {
    pub(super) fn start(
        runtime: Option<&Arc<dyn GenerationBackgroundRuntime>>,
        task_id: &str,
        user_visible: bool,
    ) -> Self {
        let runtime = runtime.and_then(|runtime| {
            if let Err(error) = runtime.start(task_id, user_visible) {
                tracing::warn!(
                    task_id,
                    "Failed to protect AI generation in background: {error}"
                );
                return None;
            }
            Some(runtime.clone())
        });

        Self {
            runtime,
            task_id: task_id.to_string(),
            last_reported_units: 0,
        }
    }

    pub(super) fn report_progress(&mut self, completed_units: u64) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        if self.last_reported_units != 0
            && completed_units.saturating_sub(self.last_reported_units) < PROGRESS_REPORT_STEP_BYTES
        {
            return;
        }

        self.last_reported_units = completed_units;
        if let Err(error) = runtime.report_progress(&self.task_id, completed_units) {
            tracing::warn!(
                task_id = self.task_id,
                "Failed to report AI generation background progress: {error}"
            );
        }
    }

    pub(super) fn complete(
        mut self,
        outcome: GenerationBackgroundOutcome,
        notify_completion: bool,
    ) {
        self.finish(outcome, notify_completion);
    }

    fn finish(&mut self, outcome: GenerationBackgroundOutcome, notify_completion: bool) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };

        if let Err(error) = runtime.finish(&self.task_id, outcome, notify_completion) {
            tracing::warn!(
                task_id = self.task_id,
                "Failed to release AI generation background protection: {error}"
            );
        }
    }
}

impl Drop for GenerationBackgroundLease {
    fn drop(&mut self) {
        self.finish(GenerationBackgroundOutcome::Cancelled, false);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tt_domain::errors::DomainError;

    use super::*;

    #[derive(Default)]
    struct RecordingRuntime {
        events: Mutex<Vec<String>>,
    }

    impl GenerationBackgroundRuntime for RecordingRuntime {
        fn start(&self, task_id: &str, _user_visible: bool) -> Result<(), DomainError> {
            self.events.lock().unwrap().push(format!("start:{task_id}"));
            Ok(())
        }

        fn report_progress(&self, task_id: &str, completed_units: u64) -> Result<(), DomainError> {
            self.events
                .lock()
                .unwrap()
                .push(format!("progress:{task_id}:{completed_units}"));
            Ok(())
        }

        fn finish(
            &self,
            task_id: &str,
            outcome: GenerationBackgroundOutcome,
            notify_completion: bool,
        ) -> Result<(), DomainError> {
            self.events
                .lock()
                .unwrap()
                .push(format!("finish:{task_id}:{outcome:?}:{notify_completion}"));
            Ok(())
        }
    }

    #[test]
    fn lease_throttles_progress_and_releases_native_runtime_once() {
        let recording = Arc::new(RecordingRuntime::default());
        let runtime: Arc<dyn GenerationBackgroundRuntime> = recording.clone();
        let mut lease = GenerationBackgroundLease::start(Some(&runtime), "request-1", true);

        lease.report_progress(1);
        lease.report_progress(PROGRESS_REPORT_STEP_BYTES);
        lease.report_progress(PROGRESS_REPORT_STEP_BYTES + 1);
        lease.report_progress(PROGRESS_REPORT_STEP_BYTES * 2 + 1);

        lease.complete(GenerationBackgroundOutcome::Succeeded, true);

        assert_eq!(
            *recording.events.lock().unwrap(),
            [
                "start:request-1",
                "progress:request-1:1",
                "progress:request-1:4097",
                "progress:request-1:8193",
                "finish:request-1:Succeeded:true",
            ]
        );
    }
}
