use std::cmp::max;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, MutexGuard, Notify};
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;
use tt_domain::errors::DomainError;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;

use crate::dto::chat_history_dto::{ChatHistoryLocator, CurrentCommitReason};
use crate::errors::ApplicationError;
use crate::services::chat_file_validation::{
    validate_character_path_component, validate_chat_file_name, validate_chat_history_locator,
};

const DEFAULT_QUIET_PERIOD: Duration = Duration::from_secs(2);
const DEFAULT_MINIMUM_INTERVAL: Duration = Duration::from_secs(5 * 60);
const DEFAULT_MAX_PENDING: usize = 32;

/// Owns automatic chat-history scheduling. Current writes only report successful
/// commits; this service coalesces them and performs history copies off the write path.
pub struct ChatHistoryCoordinator {
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    state: Mutex<CoordinatorState>,
    wake: Notify,
    execution_gate: Mutex<()>,
    quiet_period: Duration,
    minimum_interval: Duration,
    max_pending: usize,
}

impl ChatHistoryCoordinator {
    pub fn new(
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
    ) -> Self {
        Self {
            chat_repository,
            group_chat_repository,
            state: Mutex::new(CoordinatorState::new(Instant::now())),
            wake: Notify::new(),
            execution_gate: Mutex::new(()),
            quiet_period: DEFAULT_QUIET_PERIOD,
            minimum_interval: DEFAULT_MINIMUM_INTERVAL,
            max_pending: DEFAULT_MAX_PENDING,
        }
    }

    pub async fn generation_started(
        &self,
        locator: ChatHistoryLocator,
    ) -> Result<(), ApplicationError> {
        validate_chat_history_locator(&locator)?;
        let mut state = self.state.lock().await;
        let replaced_stale_generation = state.begin_generation(locator);
        drop(state);
        if replaced_stale_generation {
            tracing::warn!("Replacing stale outer chat-history generation state");
        }
        self.wake.notify_one();
        Ok(())
    }

    pub async fn generation_finished(
        &self,
        locator: ChatHistoryLocator,
    ) -> Result<(), ApplicationError> {
        validate_chat_history_locator(&locator)?;
        let mut state = self.state.lock().await;
        let outcome = state.finish_generation(
            &locator,
            Instant::now(),
            self.quiet_period,
            self.max_pending,
        )?;
        drop(state);
        if outcome == NoteOutcome::PendingCapacityReached {
            tracing::warn!(
                max_pending = self.max_pending,
                "Automatic chat-history queue is full; skipping a completed generation"
            );
        }
        self.wake.notify_one();
        Ok(())
    }

    /// Report a current commit only after its repository operation has succeeded.
    /// History scheduling remains best-effort and never changes current-write results.
    pub async fn note_current_committed(
        &self,
        locator: ChatHistoryLocator,
        reason: CurrentCommitReason,
    ) {
        let mut state = self.state.lock().await;
        let outcome = state.note_commit(
            locator,
            reason,
            Instant::now(),
            self.quiet_period,
            self.max_pending,
        );
        drop(state);

        match outcome {
            NoteOutcome::WakeWorker => self.wake.notify_one(),
            NoteOutcome::PendingCapacityReached => tracing::warn!(
                max_pending = self.max_pending,
                "Automatic chat-history queue is full; skipping a new locator"
            ),
            NoteOutcome::NoWake => {}
        }
    }

    /// Explicit history snapshots stay immediate and observable, but share the
    /// execution gate and global rate boundary with automatic snapshots.
    pub async fn backup_character_explicit(
        &self,
        character_id: &str,
        file_name: &str,
    ) -> Result<(), ApplicationError> {
        validate_character_path_component(character_id)?;
        validate_chat_file_name(file_name, "Chat file name")?;

        let locator = ChatHistoryLocator::Character {
            character_id: character_id.to_string(),
            file_name: file_name.to_string(),
        };
        let _execution_guard = self.execution_gate.lock().await;
        let committed_before = self.state.lock().await.next_commit_seq;

        self.chat_repository
            .backup_chat(character_id, file_name)
            .await?;

        let mut state = self.state.lock().await;
        state.complete_explicit(
            &locator,
            committed_before,
            Instant::now(),
            self.minimum_interval,
        );
        drop(state);
        self.wake.notify_one();
        Ok(())
    }

    pub async fn invalidate(&self, locator: &ChatHistoryLocator) {
        let mut state = self.state.lock().await;
        state.invalidate(locator);
        drop(state);
        self.wake.notify_one();
    }

    pub async fn invalidate_character(&self, character_id: &str) {
        let mut state = self.state.lock().await;
        state.invalidate_character(character_id);
        drop(state);
        self.wake.notify_one();
    }

    pub async fn invalidate_all_pending(&self) {
        let mut state = self.state.lock().await;
        state.invalidate_all_pending();
        drop(state);
        self.wake.notify_one();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn commit_sequence_for_test(&self) -> u64 {
        self.state.lock().await.next_commit_seq
    }

    pub async fn lock_snapshot_execution(&self) -> MutexGuard<'_, ()> {
        self.execution_gate.lock().await
    }

    pub async fn run(self: Arc<Self>, cancel: CancellationToken) {
        loop {
            let notified = self.wake.notified();
            tokio::pin!(notified);

            match self.worker_decision(Instant::now()).await {
                WorkerDecision::Stopped => {
                    tokio::select! {
                        _ = &mut notified => continue,
                        _ = cancel.cancelled() => break,
                    }
                }
                WorkerDecision::WaitUntil(deadline) => {
                    tokio::select! {
                        _ = sleep_until(deadline) => {}
                        _ = &mut notified => {}
                        _ = cancel.cancelled() => break,
                    }
                }
                WorkerDecision::Ready => {
                    let execution_guard = tokio::select! {
                        guard = self.execution_gate.lock() => guard,
                        _ = cancel.cancelled() => break,
                    };
                    if cancel.is_cancelled() {
                        break;
                    }

                    let attempt = {
                        let mut state = self.state.lock().await;
                        state.take_ready(Instant::now())
                    };
                    let Some(attempt) = attempt else {
                        drop(execution_guard);
                        continue;
                    };

                    let result = self.create_automatic_snapshot(&attempt.locator).await;
                    drop(execution_guard);

                    let mut state = self.state.lock().await;
                    match &result {
                        Err(DomainError::Transient(_)) => {
                            state.defer_automatic(&attempt, Instant::now() + self.quiet_period)
                        }
                        _ => state.complete_automatic(
                            &attempt,
                            Instant::now(),
                            self.minimum_interval,
                        ),
                    }
                    drop(state);

                    match result {
                        Ok(()) => {}
                        Err(DomainError::Transient(message)) => tracing::debug!(
                            locator = ?attempt.locator,
                            commit_seq = attempt.commit_seq,
                            reason = %message,
                            "Deferring busy automatic chat-history request"
                        ),
                        Err(DomainError::NotFound(message)) => tracing::debug!(
                            locator = ?attempt.locator,
                            commit_seq = attempt.commit_seq,
                            reason = %message,
                            "Skipping stale automatic chat-history request"
                        ),
                        Err(error) => tracing::error!(
                            target: tt_contracts::observability::USER_VISIBLE_ERROR,
                            locator = ?attempt.locator,
                            commit_seq = attempt.commit_seq,
                            error = %error,
                            "Automatic chat-history snapshot failed"
                        ),
                    }
                    self.wake.notify_one();
                }
            }
        }
    }

    async fn worker_decision(&self, now: Instant) -> WorkerDecision {
        self.state.lock().await.worker_decision(now)
    }

    async fn create_automatic_snapshot(
        &self,
        locator: &ChatHistoryLocator,
    ) -> Result<(), DomainError> {
        match locator {
            ChatHistoryLocator::Character {
                character_id,
                file_name,
            } => {
                self.chat_repository
                    .backup_chat_automatic(character_id, file_name)
                    .await
            }
            ChatHistoryLocator::Group { chat_id } => {
                self.group_chat_repository
                    .backup_group_chat_automatic(chat_id)
                    .await
            }
        }
    }
}

#[derive(Debug)]
struct CoordinatorState {
    active_generation: Option<ActiveGeneration>,
    pending: HashMap<ChatHistoryLocator, PendingSnapshot>,
    in_flight: Option<SnapshotAttempt>,
    next_commit_seq: u64,
    next_order: u64,
    next_automatic_at: Instant,
}

impl CoordinatorState {
    fn new(now: Instant) -> Self {
        Self {
            active_generation: None,
            pending: HashMap::new(),
            in_flight: None,
            next_commit_seq: 0,
            next_order: 0,
            next_automatic_at: now,
        }
    }

    fn begin_generation(&mut self, locator: ChatHistoryLocator) -> bool {
        let replaced_stale_generation = self.active_generation.take().is_some();
        let suspended = self.pending.remove(&locator);
        self.active_generation = Some(ActiveGeneration {
            locator,
            suspended,
            latest_commit_seq: self.next_commit_seq,
            saw_commit: false,
            saw_checkpoint: false,
        });
        replaced_stale_generation
    }

    fn finish_generation(
        &mut self,
        locator: &ChatHistoryLocator,
        now: Instant,
        quiet_period: Duration,
        max_pending: usize,
    ) -> Result<NoteOutcome, ApplicationError> {
        let Some(active) = self.active_generation.as_ref() else {
            return Err(ApplicationError::Conflict(
                "No outer generation is active".to_string(),
            ));
        };
        if &active.locator != locator {
            return Err(ApplicationError::Conflict(
                "Generation finish locator does not match its start locator".to_string(),
            ));
        }

        let active = self
            .active_generation
            .take()
            .expect("active generation was checked above");
        let outcome = if active.saw_checkpoint {
            self.schedule(
                active.locator,
                active.latest_commit_seq,
                now + quiet_period,
                max_pending,
            )
        } else if !active.saw_commit
            && let Some(suspended) = active.suspended
        {
            if self.pending.len() >= max_pending {
                NoteOutcome::PendingCapacityReached
            } else {
                self.pending.insert(active.locator, suspended);
                NoteOutcome::WakeWorker
            }
        } else {
            NoteOutcome::NoWake
        };
        Ok(outcome)
    }

    fn note_commit(
        &mut self,
        locator: ChatHistoryLocator,
        reason: CurrentCommitReason,
        now: Instant,
        quiet_period: Duration,
        max_pending: usize,
    ) -> NoteOutcome {
        self.next_commit_seq += 1;
        let commit_seq = self.next_commit_seq;

        if let Some(active) = self.active_generation.as_mut()
            && active.locator == locator
        {
            active.latest_commit_seq = commit_seq;
            active.saw_commit = true;
            active.saw_checkpoint |= reason == CurrentCommitReason::GenerationCheckpoint;
            return NoteOutcome::NoWake;
        }

        match reason {
            CurrentCommitReason::ProviderBarrier => {
                self.pending.remove(&locator);
                NoteOutcome::WakeWorker
            }
            CurrentCommitReason::Mutation | CurrentCommitReason::GenerationCheckpoint => {
                self.schedule(locator, commit_seq, now + quiet_period, max_pending)
            }
            CurrentCommitReason::Maintenance => {
                if let Some(pending) = self.pending.get_mut(&locator) {
                    pending.commit_seq = commit_seq;
                }
                NoteOutcome::NoWake
            }
        }
    }

    fn schedule(
        &mut self,
        locator: ChatHistoryLocator,
        commit_seq: u64,
        due_at: Instant,
        max_pending: usize,
    ) -> NoteOutcome {
        if let Some(pending) = self.pending.get_mut(&locator) {
            pending.commit_seq = commit_seq;
            pending.due_at = due_at;
            return NoteOutcome::WakeWorker;
        }
        if self.pending.len() >= max_pending {
            return NoteOutcome::PendingCapacityReached;
        }

        let order = self.next_order;
        self.next_order += 1;
        self.pending.insert(
            locator,
            PendingSnapshot {
                commit_seq,
                due_at,
                order,
            },
        );
        NoteOutcome::WakeWorker
    }

    fn worker_decision(&self, now: Instant) -> WorkerDecision {
        if self.active_generation.is_some() || self.pending.is_empty() {
            return WorkerDecision::Stopped;
        }

        let deadline = self
            .pending
            .values()
            .map(|pending| max(pending.due_at, self.next_automatic_at))
            .min()
            .expect("pending map is not empty");
        if deadline <= now {
            WorkerDecision::Ready
        } else {
            WorkerDecision::WaitUntil(deadline)
        }
    }

    fn take_ready(&mut self, now: Instant) -> Option<SnapshotAttempt> {
        if self.active_generation.is_some() || self.in_flight.is_some() {
            return None;
        }

        let (locator, _) = self
            .pending
            .iter()
            .filter_map(|(locator, pending)| {
                let deadline = max(pending.due_at, self.next_automatic_at);
                (deadline <= now).then_some((locator, (deadline, pending.order)))
            })
            .min_by_key(|(_, key)| *key)?;
        let locator = locator.clone();
        let pending = self
            .pending
            .remove(&locator)
            .expect("selected pending snapshot must exist");
        let attempt = SnapshotAttempt {
            locator,
            commit_seq: pending.commit_seq,
        };
        self.in_flight = Some(attempt.clone());
        Some(attempt)
    }

    fn complete_automatic(
        &mut self,
        attempt: &SnapshotAttempt,
        now: Instant,
        minimum_interval: Duration,
    ) {
        let current = self
            .in_flight
            .take()
            .expect("automatic completion requires an in-flight snapshot");
        assert_eq!(&current, attempt, "automatic snapshot completion mismatch");
        self.next_automatic_at = max(self.next_automatic_at, now + minimum_interval);
    }

    fn defer_automatic(&mut self, attempt: &SnapshotAttempt, retry_at: Instant) {
        let current = self
            .in_flight
            .take()
            .expect("automatic deferral requires an in-flight snapshot");
        assert_eq!(&current, attempt, "automatic snapshot deferral mismatch");

        if self.pending.contains_key(&attempt.locator) {
            return;
        }

        // The in-flight attempt owns one reserved slot, so a deferral stays bounded
        // at max_pending + 1 even if another locator filled the pending map meanwhile.
        let order = self.next_order;
        self.next_order += 1;
        self.pending.insert(
            attempt.locator.clone(),
            PendingSnapshot {
                commit_seq: attempt.commit_seq,
                due_at: retry_at,
                order,
            },
        );
    }

    fn complete_explicit(
        &mut self,
        locator: &ChatHistoryLocator,
        committed_before: u64,
        now: Instant,
        minimum_interval: Duration,
    ) {
        if self
            .pending
            .get(locator)
            .is_some_and(|pending| pending.commit_seq <= committed_before)
        {
            self.pending.remove(locator);
        }
        if let Some(active) = self.active_generation.as_mut()
            && &active.locator == locator
            && active
                .suspended
                .as_ref()
                .is_some_and(|pending| pending.commit_seq <= committed_before)
        {
            active.suspended = None;
        }
        self.next_automatic_at = max(self.next_automatic_at, now + minimum_interval);
    }

    fn invalidate(&mut self, locator: &ChatHistoryLocator) {
        self.pending.remove(locator);
        if let Some(active) = self.active_generation.as_mut()
            && &active.locator == locator
        {
            active.suspended = None;
            active.saw_checkpoint = false;
        }
    }

    fn invalidate_character(&mut self, character_id: &str) {
        self.pending.retain(|locator, _| {
            !matches!(
                locator,
                ChatHistoryLocator::Character {
                    character_id: pending_character_id,
                    ..
                } if pending_character_id == character_id
            )
        });
        if let Some(active) = self.active_generation.as_mut()
            && matches!(
                &active.locator,
                ChatHistoryLocator::Character {
                    character_id: active_character_id,
                    ..
                } if active_character_id == character_id
            )
        {
            active.suspended = None;
            active.saw_checkpoint = false;
        }
    }

    fn invalidate_all_pending(&mut self) {
        self.pending.clear();
        if let Some(active) = self.active_generation.as_mut() {
            active.suspended = None;
            active.saw_checkpoint = false;
        }
    }
}

#[derive(Debug)]
struct ActiveGeneration {
    locator: ChatHistoryLocator,
    suspended: Option<PendingSnapshot>,
    latest_commit_seq: u64,
    saw_commit: bool,
    saw_checkpoint: bool,
}

#[derive(Clone, Debug)]
struct PendingSnapshot {
    commit_seq: u64,
    due_at: Instant,
    order: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SnapshotAttempt {
    locator: ChatHistoryLocator,
    commit_seq: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoteOutcome {
    WakeWorker,
    NoWake,
    PendingCapacityReached,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerDecision {
    Stopped,
    WaitUntil(Instant),
    Ready,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(file_name: &str) -> ChatHistoryLocator {
        ChatHistoryLocator::Character {
            character_id: "alice".to_string(),
            file_name: file_name.to_string(),
        }
    }

    #[test]
    fn provider_only_generation_does_not_schedule_history() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);

        state.begin_generation(locator.clone());
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::ProviderBarrier,
            now,
            Duration::from_secs(2),
            32,
        );
        state
            .finish_generation(&locator, now, Duration::from_secs(2), 32)
            .unwrap();

        assert!(state.pending.is_empty());
    }

    #[test]
    fn checkpoint_and_outer_finish_schedule_the_latest_commit() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);

        state.begin_generation(locator.clone());
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::GenerationCheckpoint,
            now,
            Duration::from_secs(2),
            32,
        );
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::from_secs(2),
            32,
        );
        state
            .finish_generation(&locator, now, Duration::from_secs(2), 32)
            .unwrap();

        let pending = state.pending.get(&locator).unwrap();
        assert_eq!(pending.commit_seq, 2);
        assert_eq!(pending.due_at, now + Duration::from_secs(2));
    }

    #[test]
    fn generation_without_a_commit_restores_existing_pending_request() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::from_secs(2),
            32,
        );
        let expected_due = state.pending[&locator].due_at;

        state.begin_generation(locator.clone());
        state
            .finish_generation(&locator, now, Duration::from_secs(2), 32)
            .unwrap();

        assert_eq!(state.pending[&locator].due_at, expected_due);
    }

    #[test]
    fn non_checkpoint_commit_invalidates_pre_generation_pending() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );

        state.begin_generation(locator.clone());
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::ProviderBarrier,
            now,
            Duration::ZERO,
            32,
        );
        state
            .finish_generation(&locator, now, Duration::ZERO, 32)
            .unwrap();

        assert!(state.pending.is_empty());
    }

    #[test]
    fn active_generation_globally_stops_worker_admission() {
        let now = Instant::now();
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            character("waiting"),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        state.begin_generation(character("active"));

        assert_eq!(state.worker_decision(now), WorkerDecision::Stopped);
        assert!(state.take_ready(now).is_none());
    }

    #[test]
    fn unrelated_commit_waits_without_being_lost_during_generation() {
        let now = Instant::now();
        let active = character("active");
        let waiting = character("waiting");
        let mut state = CoordinatorState::new(now);
        state.begin_generation(active.clone());

        state.note_commit(
            waiting.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );

        assert!(state.pending.contains_key(&waiting));
        assert_eq!(state.worker_decision(now), WorkerDecision::Stopped);
        state
            .finish_generation(&active, now, Duration::ZERO, 32)
            .unwrap();
        assert_eq!(state.worker_decision(now), WorkerDecision::Ready);
    }

    #[test]
    fn new_generation_replaces_stale_generation_state() {
        let now = Instant::now();
        let stale = character("stale");
        let current = character("current");
        let mut state = CoordinatorState::new(now);
        assert!(!state.begin_generation(stale));

        assert!(state.begin_generation(current.clone()));
        state.note_commit(
            current.clone(),
            CurrentCommitReason::GenerationCheckpoint,
            now,
            Duration::ZERO,
            32,
        );
        state
            .finish_generation(&current, now, Duration::ZERO, 32)
            .unwrap();

        assert!(state.pending.contains_key(&current));
    }

    #[test]
    fn maintenance_only_refreshes_an_existing_pending_request() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);

        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Maintenance,
            now,
            Duration::from_secs(30),
            32,
        );
        assert!(state.pending.is_empty());

        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::from_secs(10),
            32,
        );
        let due_at = state.pending[&locator].due_at;
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Maintenance,
            now + Duration::from_secs(1),
            Duration::from_secs(30),
            32,
        );

        assert_eq!(state.pending[&locator].commit_seq, 3);
        assert_eq!(state.pending[&locator].due_at, due_at);
    }

    #[test]
    fn repeated_commits_are_latest_wins_without_growing_the_map() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now + Duration::from_secs(1),
            Duration::ZERO,
            32,
        );

        assert_eq!(state.pending.len(), 1);
        assert_eq!(state.pending[&locator].commit_seq, 2);
    }

    #[test]
    fn global_interval_applies_after_an_attempt_even_with_trailing_work() {
        let now = Instant::now();
        let first = character("first");
        let second = character("second");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            first,
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        state.note_commit(
            second,
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );

        let attempt = state.take_ready(now).unwrap();
        state.complete_automatic(&attempt, now, Duration::from_secs(60));

        assert_eq!(
            state.worker_decision(now),
            WorkerDecision::WaitUntil(now + Duration::from_secs(60))
        );
    }

    #[test]
    fn explicit_completion_preserves_a_newer_commit() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        let explicit_started_at = state.next_commit_seq;
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );

        state.complete_explicit(&locator, explicit_started_at, now, Duration::from_secs(60));

        assert_eq!(state.pending[&locator].commit_seq, 2);
    }

    #[test]
    fn explicit_completion_clears_an_older_suspended_request() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        let explicit_started_at = state.next_commit_seq;
        state.begin_generation(locator.clone());

        state.complete_explicit(&locator, explicit_started_at, now, Duration::from_secs(60));
        state
            .finish_generation(&locator, now, Duration::ZERO, 32)
            .unwrap();

        assert!(state.pending.is_empty());
    }

    #[test]
    fn in_flight_commit_creates_a_trailing_latest_request() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        let attempt = state.take_ready(now).unwrap();

        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        state.complete_automatic(&attempt, now, Duration::from_secs(60));

        assert_eq!(state.pending[&locator].commit_seq, 2);
    }

    #[test]
    fn transient_attempt_is_deferred_without_consuming_the_global_interval() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        let attempt = state.take_ready(now).unwrap();

        state.defer_automatic(&attempt, now + Duration::from_secs(2));

        assert_eq!(state.pending[&locator].commit_seq, attempt.commit_seq);
        assert_eq!(
            state.worker_decision(now),
            WorkerDecision::WaitUntil(now + Duration::from_secs(2))
        );
        assert_eq!(state.next_automatic_at, now);
    }

    #[test]
    fn transient_attempt_does_not_overwrite_a_newer_trailing_request() {
        let now = Instant::now();
        let locator = character("chat");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        let attempt = state.take_ready(now).unwrap();
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );

        state.defer_automatic(&attempt, now + Duration::from_secs(2));

        assert_eq!(state.pending[&locator].commit_seq, 2);
        assert_eq!(state.pending[&locator].due_at, now);
    }

    #[test]
    fn pending_capacity_still_allows_existing_locator_coalescing() {
        let now = Instant::now();
        let first = character("first");
        let second = character("second");
        let mut state = CoordinatorState::new(now);

        assert_eq!(
            state.note_commit(
                first.clone(),
                CurrentCommitReason::Mutation,
                now,
                Duration::ZERO,
                1,
            ),
            NoteOutcome::WakeWorker
        );
        assert_eq!(
            state.note_commit(
                second,
                CurrentCommitReason::Mutation,
                now,
                Duration::ZERO,
                1,
            ),
            NoteOutcome::PendingCapacityReached
        );
        assert_eq!(
            state.note_commit(
                first.clone(),
                CurrentCommitReason::Mutation,
                now,
                Duration::ZERO,
                1,
            ),
            NoteOutcome::WakeWorker
        );
        assert_eq!(state.pending.len(), 1);
        assert_eq!(state.pending[&first].commit_seq, 3);
    }

    #[test]
    fn invalidate_all_preserves_generation_gate_and_global_cooldown() {
        let now = Instant::now();
        let active = character("active");
        let waiting = character("waiting");
        let mut state = CoordinatorState::new(now);
        state.next_automatic_at = now + Duration::from_secs(60);
        state.note_commit(
            waiting,
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            32,
        );
        state.begin_generation(active.clone());

        state.invalidate_all_pending();

        assert!(state.pending.is_empty());
        assert_eq!(state.next_automatic_at, now + Duration::from_secs(60));
        assert_eq!(state.active_generation.as_ref().unwrap().locator, active);
    }

    #[test]
    fn invalidation_revokes_an_active_checkpoint() {
        let now = Instant::now();
        let locator = character("active");
        let mut state = CoordinatorState::new(now);
        state.begin_generation(locator.clone());
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::GenerationCheckpoint,
            now,
            Duration::ZERO,
            32,
        );

        state.invalidate(&locator);
        state
            .finish_generation(&locator, now, Duration::ZERO, 32)
            .unwrap();

        assert!(state.pending.is_empty());
    }

    #[test]
    fn a_new_checkpoint_after_invalidation_authorizes_history_again() {
        let now = Instant::now();
        let locator = character("active");
        let mut state = CoordinatorState::new(now);
        state.begin_generation(locator.clone());
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::GenerationCheckpoint,
            now,
            Duration::ZERO,
            32,
        );
        state.invalidate(&locator);

        state.note_commit(
            locator.clone(),
            CurrentCommitReason::GenerationCheckpoint,
            now,
            Duration::ZERO,
            32,
        );
        state
            .finish_generation(&locator, now, Duration::ZERO, 32)
            .unwrap();

        assert_eq!(state.pending[&locator].commit_seq, 2);
    }

    #[test]
    fn completed_generation_reports_a_full_pending_map() {
        let now = Instant::now();
        let locator = character("active");
        let mut state = CoordinatorState::new(now);
        state.note_commit(
            character("waiting"),
            CurrentCommitReason::Mutation,
            now,
            Duration::ZERO,
            1,
        );
        state.begin_generation(locator.clone());
        state.note_commit(
            locator.clone(),
            CurrentCommitReason::GenerationCheckpoint,
            now,
            Duration::ZERO,
            1,
        );

        assert_eq!(
            state
                .finish_generation(&locator, now, Duration::ZERO, 1)
                .unwrap(),
            NoteOutcome::PendingCapacityReached,
        );
    }
}
