use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::{Mutex, Notify, RwLock, watch};
use tt_domain::models::upstream_failure::UpstreamFailure;

use crate::dto::chat_completion_dto::{
    ChatCompletionStreamEventDto, ChatCompletionStreamReadResultDto, ChatCompletionStreamStatusDto,
};
use crate::errors::ApplicationError;

const READ_BATCH_SIZE: usize = 64;
const MAX_REPLAY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct StreamSessionRegistry {
    active: RwLock<HashMap<String, Arc<StreamSession>>>,
}

impl StreamSessionRegistry {
    pub(super) async fn register(
        &self,
        stream_id: &str,
    ) -> Result<watch::Receiver<bool>, ApplicationError> {
        let mut active = self.active.write().await;
        if active.contains_key(stream_id) {
            return Err(ApplicationError::Conflict(format!(
                "Chat completion stream already exists: {stream_id}"
            )));
        }

        let (session, cancel) = StreamSession::new();
        active.insert(stream_id.to_string(), Arc::new(session));
        Ok(cancel)
    }

    pub(super) async fn read(
        &self,
        stream_id: &str,
        after_seq: u64,
    ) -> Result<ChatCompletionStreamReadResultDto, ApplicationError> {
        let session = self
            .active
            .read()
            .await
            .get(stream_id)
            .cloned()
            .ok_or_else(|| {
                ApplicationError::NotFound(format!("Chat completion stream not found: {stream_id}"))
            })?;

        session.read(after_seq).await
    }

    pub(super) async fn append(
        &self,
        stream_id: &str,
        data: String,
    ) -> Result<bool, ApplicationError> {
        let Some(session) = self.active.read().await.get(stream_id).cloned() else {
            return Ok(false);
        };

        session.append(data).await
    }

    pub(super) async fn finish(&self, stream_id: &str) {
        if let Some(session) = self.active.read().await.get(stream_id).cloned() {
            session.finish().await;
        }
    }

    pub(super) async fn fail(&self, stream_id: &str, error: ApplicationError) {
        let Some(session) = self.active.read().await.get(stream_id).cloned() else {
            return;
        };
        let details = match &error {
            ApplicationError::UpstreamFailure(failure) => Some(failure.clone()),
            _ => None,
        };
        session.fail(error.to_string(), details).await;
    }

    pub(super) async fn remove(&self, stream_id: &str) -> bool {
        let session = self.active.write().await.remove(stream_id);
        let Some(session) = session else {
            return false;
        };

        session.cancel().await;
        true
    }
}

struct StreamSession {
    state: Mutex<StreamSessionState>,
    changed: Notify,
    cancel: watch::Sender<bool>,
}

impl StreamSession {
    fn new() -> (Self, watch::Receiver<bool>) {
        let (cancel, receiver) = watch::channel(false);
        (
            Self {
                state: Mutex::new(StreamSessionState::default()),
                changed: Notify::new(),
                cancel,
            },
            receiver,
        )
    }

    fn wake_reader(&self) {
        // A stream has one cursor reader; notify_one retains a permit across the check/wait race.
        self.changed.notify_one();
    }

    async fn append(&self, data: String) -> Result<bool, ApplicationError> {
        let mut state = self.state.lock().await;
        if state.status != ChatCompletionStreamStatusDto::Running {
            return Ok(false);
        }
        if state.buffered_bytes.saturating_add(data.len()) > MAX_REPLAY_BYTES {
            return Err(ApplicationError::InternalError(format!(
                "Chat completion stream replay buffer exceeded {} MiB",
                MAX_REPLAY_BYTES / 1024 / 1024
            )));
        }

        let seq = state.next_seq;
        state.next_seq += 1;
        state.buffered_bytes += data.len();
        state
            .events
            .push_back(ChatCompletionStreamEventDto::Chunk { seq, data });
        drop(state);
        self.wake_reader();
        Ok(true)
    }

    async fn finish(&self) {
        let mut state = self.state.lock().await;
        if state.status != ChatCompletionStreamStatusDto::Running {
            return;
        }

        let seq = state.next_seq;
        state.next_seq += 1;
        state
            .events
            .push_back(ChatCompletionStreamEventDto::Done { seq });
        state.status = ChatCompletionStreamStatusDto::Done;
        drop(state);
        self.wake_reader();
    }

    async fn fail(&self, message: String, details: Option<UpstreamFailure>) {
        let mut state = self.state.lock().await;
        if state.status != ChatCompletionStreamStatusDto::Running {
            return;
        }

        let seq = state.next_seq;
        state.next_seq += 1;
        state.events.push_back(ChatCompletionStreamEventDto::Error {
            seq,
            message,
            details,
        });
        state.status = ChatCompletionStreamStatusDto::Error;
        drop(state);
        self.wake_reader();
    }

    async fn cancel(&self) {
        let mut state = self.state.lock().await;
        if state.status == ChatCompletionStreamStatusDto::Running {
            state.status = ChatCompletionStreamStatusDto::Cancelled;
        }
        drop(state);
        let _ = self.cancel.send(true);
        self.wake_reader();
    }

    async fn read(
        &self,
        after_seq: u64,
    ) -> Result<ChatCompletionStreamReadResultDto, ApplicationError> {
        loop {
            let notified = self.changed.notified();
            let mut state = self.state.lock().await;

            if after_seq < state.acknowledged_seq {
                return Err(ApplicationError::Conflict(format!(
                    "Chat completion stream cursor {after_seq} is behind acknowledged sequence {}",
                    state.acknowledged_seq
                )));
            }
            if after_seq > state.delivered_seq {
                return Err(ApplicationError::ValidationError(format!(
                    "Chat completion stream cursor {after_seq} exceeds delivered sequence {}",
                    state.delivered_seq
                )));
            }

            while state
                .events
                .front()
                .is_some_and(|event| event.seq() <= after_seq)
            {
                if let Some(event) = state.events.pop_front() {
                    state.buffered_bytes =
                        state.buffered_bytes.saturating_sub(event.buffered_bytes());
                }
            }
            state.acknowledged_seq = after_seq;

            let events = state
                .events
                .iter()
                .take(READ_BATCH_SIZE)
                .cloned()
                .collect::<Vec<_>>();
            if let Some(event) = events.last() {
                state.delivered_seq = state.delivered_seq.max(event.seq());
            }

            if !events.is_empty() || state.status != ChatCompletionStreamStatusDto::Running {
                return Ok(ChatCompletionStreamReadResultDto {
                    events,
                    status: state.status,
                });
            }

            drop(state);
            notified.await;
        }
    }
}

struct StreamSessionState {
    events: VecDeque<ChatCompletionStreamEventDto>,
    status: ChatCompletionStreamStatusDto,
    next_seq: u64,
    delivered_seq: u64,
    acknowledged_seq: u64,
    buffered_bytes: usize,
}

impl Default for StreamSessionState {
    fn default() -> Self {
        Self {
            events: VecDeque::new(),
            status: ChatCompletionStreamStatusDto::Running,
            next_seq: 1,
            delivered_seq: 0,
            acknowledged_seq: 0,
            buffered_bytes: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn unacknowledged_stream_events_are_replayed_in_sequence() {
        let session = Arc::new(StreamSession::new().0);
        let waiting = tokio::spawn({
            let session = session.clone();
            async move { session.read(0).await }
        });
        tokio::task::yield_now().await;
        session.append("one".to_string()).await.unwrap();

        let first = timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let replay = session.read(0).await.unwrap();
        assert_eq!(replay, first);

        session.append("two".to_string()).await.unwrap();
        let second = session.read(1).await.unwrap();
        assert_eq!(second.events[0].seq(), 2);

        session.finish().await;
        let terminal = session.read(2).await.unwrap();
        assert!(matches!(
            terminal.events.as_slice(),
            [ChatCompletionStreamEventDto::Done { seq: 3 }]
        ));

        let acknowledged = session.read(3).await.unwrap();
        assert!(acknowledged.events.is_empty());
        assert_eq!(acknowledged.status, ChatCompletionStreamStatusDto::Done);
    }
}
