use tt_domain::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationBackgroundOutcome {
    Succeeded,
    Failed { status_code: Option<u16> },
    Cancelled,
}

pub trait GenerationBackgroundRuntime: Send + Sync {
    fn start(&self, task_id: &str, user_visible: bool) -> Result<(), DomainError>;

    fn report_progress(&self, _task_id: &str, _completed_units: u64) -> Result<(), DomainError> {
        Ok(())
    }

    fn finish(
        &self,
        task_id: &str,
        outcome: GenerationBackgroundOutcome,
        notify_completion: bool,
    ) -> Result<(), DomainError>;
}
