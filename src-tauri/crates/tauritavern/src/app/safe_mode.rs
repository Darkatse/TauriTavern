use std::path::Path;
use std::sync::Mutex;

use tt_adapter_storage_core::FileLaunchRecordRepository;
use tt_contracts::safe_mode::{SafeModePrompt, SafeModeReport};
use tt_domain::errors::DomainError;
use tt_domain::models::safe_mode::{LaunchDecision, LaunchRecord};

pub(crate) struct SafeMode {
    repository: FileLaunchRecordRepository,
    rescue: bool,
    state: Mutex<State>,
}

struct State {
    record: LaunchRecord,
    prompt: Option<SafeModePrompt>,
}

impl SafeMode {
    /// A record that cannot be read or saved never stops startup, and a launch it cannot record
    /// decides nothing: a request whose consumption is not saved would rescue every launch.
    pub(crate) fn register_launch(app_root: &Path) -> Self {
        let repository = FileLaunchRecordRepository::new(app_root);
        let mut record = repository.load().unwrap_or_else(|error| {
            tracing::warn!(%error, "Starting a new safe mode launch record");
            LaunchRecord::default()
        });
        let decision = record.register(chrono::Utc::now().timestamp_millis());
        let decision = match repository.save(&record) {
            Ok(()) => decision,
            Err(error) => {
                tracing::warn!(%error, ?decision, "Failed to save the safe mode launch record");
                LaunchDecision::Normal
            }
        };
        if decision != LaunchDecision::Normal {
            tracing::info!(?decision, "Safe mode");
        }

        Self {
            repository,
            rescue: decision == LaunchDecision::Rescue,
            state: Mutex::new(State {
                record,
                prompt: (decision == LaunchDecision::Offer).then_some(SafeModePrompt::Offer),
            }),
        }
    }

    pub(crate) fn rescue_requested(&self) -> bool {
        self.rescue
    }

    pub(crate) fn report(&self, report: SafeModeReport) {
        self.lock().prompt = Some(SafeModePrompt::Report(report));
    }

    pub(crate) fn prompt(&self) -> Option<SafeModePrompt> {
        self.lock().prompt.clone()
    }

    /// A dismissal that is not saved only offers again on a later launch.
    pub(crate) fn dismiss_prompt(&self) {
        let mut state = self.lock();
        if let Some(SafeModePrompt::Offer) = state.prompt.take() {
            state.record.dismiss_offer();
            if let Err(error) = self.repository.save(&state.record) {
                tracing::warn!(%error, "Failed to save the declined safe mode offer");
            }
        }
    }

    /// A request that is not saved fails and changes nothing.
    pub(crate) fn request(&self) -> Result<(), DomainError> {
        let mut state = self.lock();
        let mut record = state.record.clone();
        record.request();
        self.repository.save(&record)?;
        state.record = record;
        state.prompt = None;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("safe mode state poisoned")
    }
}
