use crate::errors::DomainError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DataArchiveLocalMutationSummary {
    pub files_written: usize,
    pub bytes_written: u64,
    pub target_changed: bool,
}

impl DataArchiveLocalMutationSummary {
    pub fn changed(&self) -> bool {
        self.files_written > 0 || self.bytes_written > 0 || self.target_changed
    }

    pub fn mark_target_changed(&mut self) {
        self.target_changed = true;
    }

    pub fn record_file_written(&mut self, bytes_written: u64) {
        self.files_written = self.files_written.saturating_add(1);
        self.bytes_written = self.bytes_written.saturating_add(bytes_written);
        self.mark_target_changed();
    }
}

#[derive(Debug)]
pub struct DataArchiveImportFailure {
    pub error: DomainError,
    pub local_applied: DataArchiveLocalMutationSummary,
}

impl DataArchiveImportFailure {
    pub fn new(error: DomainError, local_applied: DataArchiveLocalMutationSummary) -> Self {
        Self {
            error,
            local_applied,
        }
    }

    pub fn without_local_mutation(error: DomainError) -> Self {
        Self::new(error, DataArchiveLocalMutationSummary::default())
    }
}

impl From<DomainError> for DataArchiveImportFailure {
    fn from(error: DomainError) -> Self {
        Self::without_local_mutation(error)
    }
}
