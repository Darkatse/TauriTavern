use std::collections::VecDeque;
use std::path::Path;

use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::FileAgentRepository;
use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentRunEvent;
use tt_ports::repositories::agent_run_repository::{
    AgentRunEventReadQuery, event_belongs_to_invocation,
};

impl FileAgentRepository {
    pub(super) async fn read_event_page(
        &self,
        run_id: &str,
        query: AgentRunEventReadQuery,
    ) -> Result<Vec<AgentRunEvent>, DomainError> {
        if query.before_seq.is_none()
            && let Some(after_seq) = query.after_seq
            && self
                .event_sequences
                .lock()
                .await
                .get(run_id)
                .is_some_and(|last_seq| *last_seq <= after_seq)
        {
            return Ok(Vec::new());
        }

        let events_path = self.load_run_dir(run_id).await?.join("events.jsonl");
        let Some(mut reader) = open_event_reader(&events_path).await? else {
            return Ok(Vec::new());
        };
        let limit = query.limit.clamp(1, 500);
        let before_seq = query.before_seq;
        let after_seq = query.after_seq.unwrap_or(0);
        let invocation_id = query.invocation_id.as_deref();
        let mut events = VecDeque::with_capacity(limit);
        let mut line = String::new();
        // append_event writes one non-empty line for each contiguous sequence number, so
        // consumed lines can be skipped without deserializing their payloads.
        let mut line_seq = 0_u64;

        while read_event_line(&mut reader, &mut line, &events_path).await? {
            if line.trim().is_empty() {
                continue;
            }

            line_seq += 1;
            if before_seq.is_some_and(|seq| line_seq >= seq) {
                break;
            }
            if before_seq.is_none() && line_seq <= after_seq {
                continue;
            }

            let event = parse_event(&line, &events_path)?;
            ensure_event_seq(&event, line_seq, &events_path)?;
            if invocation_id.is_some_and(|id| !event_belongs_to_invocation(&event, id)) {
                continue;
            }

            if before_seq.is_some() {
                if events.len() == limit {
                    events.pop_front();
                }
                events.push_back(event);
            } else {
                events.push_back(event);
                if events.len() == limit {
                    break;
                }
            }
        }

        Ok(events.into_iter().collect())
    }

    pub(super) async fn read_all_events(
        &self,
        run_id: &str,
    ) -> Result<Vec<AgentRunEvent>, DomainError> {
        let events_path = self.load_run_dir(run_id).await?.join("events.jsonl");
        let Some(mut reader) = open_event_reader(&events_path).await? else {
            return Ok(Vec::new());
        };
        let mut events = Vec::new();
        let mut line = String::new();

        while read_event_line(&mut reader, &mut line, &events_path).await? {
            if !line.trim().is_empty() {
                let event = parse_event(&line, &events_path)?;
                ensure_event_seq(&event, events.len() as u64 + 1, &events_path)?;
                events.push(event);
            }
        }

        Ok(events)
    }
}

async fn open_event_reader(path: &Path) -> Result<Option<BufReader<File>>, DomainError> {
    match File::open(path).await {
        Ok(file) => Ok(Some(BufReader::new(file))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(DomainError::InternalError(format!(
            "Failed to read agent event journal {}: {}",
            path.display(),
            error
        ))),
    }
}

async fn read_event_line(
    reader: &mut BufReader<File>,
    line: &mut String,
    path: &Path,
) -> Result<bool, DomainError> {
    line.clear();
    reader
        .read_line(line)
        .await
        .map(|bytes| bytes != 0)
        .map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent event journal {}: {}",
                path.display(),
                error
            ))
        })
}

fn parse_event(line: &str, path: &Path) -> Result<AgentRunEvent, DomainError> {
    serde_json::from_str(line).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid agent event in {}: {}",
            path.display(),
            error
        ))
    })
}

fn ensure_event_seq(event: &AgentRunEvent, expected: u64, path: &Path) -> Result<(), DomainError> {
    if event.seq != expected {
        return Err(DomainError::InvalidData(format!(
            "Invalid agent event sequence in {}: expected {}, found {}",
            path.display(),
            expected,
            event.seq
        )));
    }
    Ok(())
}
