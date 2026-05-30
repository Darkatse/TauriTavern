use tokio::fs;

use super::FileAgentRepository;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::AgentRunEvent;

impl FileAgentRepository {
    pub(super) async fn read_all_events(
        &self,
        run_id: &str,
    ) -> Result<Vec<AgentRunEvent>, DomainError> {
        let events_path = self.load_run_dir(run_id).await?.join("events.jsonl");
        let contents = match fs::read_to_string(&events_path).await {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read agent event journal {}: {}",
                    events_path.display(),
                    error
                )));
            }
        };

        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<AgentRunEvent>(line).map_err(|error| {
                    DomainError::InvalidData(format!(
                        "Invalid agent event in {}: {}",
                        events_path.display(),
                        error
                    ))
                })
            })
            .collect()
    }
}
