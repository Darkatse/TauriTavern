use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::Notify;

use tt_domain::errors::DomainError;
use tt_domain::ios_policy::{IosPolicyScope, resolve_ios_policy_activation_report};
use tt_domain::models::secret::Secrets;
use tt_ports::repositories::horde_repository::HordeRepository;
use tt_ports::repositories::secret_repository::SecretRepository;

use super::{ANONYMOUS_API_KEY, HordeService, text};
use crate::dto::horde_dto::HordeTextGenerationRequestDto;
use crate::errors::ApplicationError;
use crate::services::generation_runtime::GenerationRuntime;

#[derive(Default)]
struct FakeHorde {
    statuses: Mutex<VecDeque<Value>>,
    submitted: Mutex<Vec<(Value, String)>>,
    cancelled: Mutex<Vec<String>>,
    status_read: Notify,
    cancel_done: Notify,
    models: Value,
    reference: Option<Value>,
}

impl FakeHorde {
    fn with_statuses(statuses: impl IntoIterator<Item = Value>) -> Self {
        Self {
            statuses: Mutex::new(statuses.into_iter().collect()),
            ..Self::default()
        }
    }
}

#[async_trait]
impl HordeRepository for FakeHorde {
    async fn heartbeat(&self) -> Result<(), DomainError> {
        unimplemented!()
    }

    async fn text_workers(&self) -> Result<Value, DomainError> {
        unimplemented!()
    }

    async fn text_models(&self) -> Result<Value, DomainError> {
        Ok(self.models.clone())
    }

    async fn text_model_reference(&self) -> Result<Value, DomainError> {
        self.reference
            .clone()
            .ok_or_else(|| DomainError::InternalError("unreachable".to_string()))
    }

    async fn find_user(&self, _api_key: &str) -> Result<Value, DomainError> {
        unimplemented!()
    }

    async fn shared_key(&self, _api_key: &str) -> Result<Option<Value>, DomainError> {
        unimplemented!()
    }

    async fn submit_text_generation(
        &self,
        body: &Value,
        api_key: &str,
    ) -> Result<Value, DomainError> {
        self.submitted
            .lock()
            .unwrap()
            .push((body.clone(), api_key.to_string()));
        Ok(json!({ "id": "task-1", "kudos": 10 }))
    }

    async fn text_generation_status(&self, _task_id: &str) -> Result<Value, DomainError> {
        let status = self.statuses.lock().unwrap().pop_front();
        self.status_read.notify_one();
        Ok(status.unwrap_or_else(|| json!({ "done": false, "queue_position": 3 })))
    }

    async fn cancel_text_generation(&self, task_id: &str) -> Result<(), DomainError> {
        self.cancelled.lock().unwrap().push(task_id.to_string());
        self.cancel_done.notify_one();
        Ok(())
    }
}

struct NoSecrets;

#[async_trait]
impl SecretRepository for NoSecrets {
    async fn save(&self, _secrets: &Secrets) -> Result<(), DomainError> {
        unimplemented!()
    }

    async fn load(&self) -> Result<Secrets, DomainError> {
        unimplemented!()
    }

    async fn clear_cache(&self) -> Result<(), DomainError> {
        unimplemented!()
    }

    async fn write_secret(
        &self,
        _key: &str,
        _value: &str,
        _label: &str,
    ) -> Result<String, DomainError> {
        unimplemented!()
    }

    async fn read_secret(
        &self,
        _key: &str,
        _id: Option<&str>,
    ) -> Result<Option<String>, DomainError> {
        Ok(None)
    }

    async fn delete_secret(&self, _key: &str, _id: Option<&str>) -> Result<(), DomainError> {
        unimplemented!()
    }

    async fn rotate_secret(&self, _key: &str, _id: &str) -> Result<(), DomainError> {
        unimplemented!()
    }

    async fn rename_secret(&self, _key: &str, _id: &str, _label: &str) -> Result<(), DomainError> {
        unimplemented!()
    }
}

fn service(horde: Arc<FakeHorde>, runtime: Arc<GenerationRuntime>) -> HordeService {
    let policy = resolve_ios_policy_activation_report(IosPolicyScope::Ignored, None, None).unwrap();
    HordeService::new(horde, Arc::new(NoSecrets), runtime, policy)
}

async fn follow(horde: &FakeHorde) -> (Result<(), ApplicationError>, Vec<String>) {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let result = text::follow_task(horde, "task-1", &sender).await;
    drop(sender);
    let mut events = Vec::new();
    while let Some(event) = receiver.recv().await {
        events.push(event);
    }
    (result, events)
}

#[tokio::test(start_paused = true)]
async fn a_text_task_streams_each_status_and_ends_after_its_generation() {
    let statuses = [
        json!({ "done": false, "queue_position": 2 }),
        json!({ "done": false, "queue_position": 0, "processing": 1 }),
        json!({ "done": true, "generations": [{ "text": "Hello", "worker_name": "w", "model": "m" }] }),
    ];
    let horde = FakeHorde::with_statuses(statuses.clone());

    let (result, events) = follow(&horde).await;

    result.unwrap();
    let mut expected: Vec<String> = statuses.iter().map(Value::to_string).collect();
    expected.push("[DONE]".to_string());
    assert_eq!(events, expected);
}

#[tokio::test(start_paused = true)]
async fn a_text_task_fails_when_faulted_unservable_or_unfinished() {
    for (statuses, expected) in [
        (vec![json!({ "faulted": true })], "faulted"),
        (vec![json!({ "is_possible": false })], "no Horde workers"),
        (
            vec![json!({ "done": true, "generations": [] })],
            "without a result",
        ),
        (Vec::new(), "timed out"),
    ] {
        let horde = FakeHorde::with_statuses(statuses);

        let (result, events) = follow(&horde).await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!events.contains(&"[DONE]".to_string()));
    }
}

#[tokio::test]
async fn a_text_generation_sends_the_page_request_and_cancelling_deletes_its_task() {
    let horde = Arc::new(FakeHorde::default());
    let runtime = Arc::new(GenerationRuntime::new(None, true));
    let service = service(horde.clone(), runtime.clone());
    let request = json!({ "prompt": "Hi", "params": { "max_length": 80 }, "models": ["m"] });
    let mut payload = request.as_object().unwrap().clone();
    payload.insert("type".to_string(), json!("normal"));

    service
        .start_text_stream(
            "stream-1".to_string(),
            HordeTextGenerationRequestDto { payload },
        )
        .await
        .unwrap();
    horde.status_read.notified().await;
    assert!(runtime.close_stream("stream-1").await);
    horde.cancel_done.notified().await;

    assert_eq!(
        *horde.submitted.lock().unwrap(),
        [(request, ANONYMOUS_API_KEY.to_string())]
    );
    assert_eq!(*horde.cancelled.lock().unwrap(), ["task-1"]);
}

#[tokio::test]
async fn text_models_merge_their_reference_entry_and_list_without_a_reference() {
    let models = json!([
        { "name": "listed", "count": 2 },
        { "name": "unlisted", "count": 1 },
    ]);
    let reference = json!({ "listed": { "display_name": "Listed", "tags": ["popular"] } });
    let runtime = Arc::new(GenerationRuntime::new(None, true));

    let with_reference = service(
        Arc::new(FakeHorde {
            models: models.clone(),
            reference: Some(reference),
            ..FakeHorde::default()
        }),
        runtime.clone(),
    );
    assert_eq!(
        with_reference.text_models(false).await.unwrap(),
        json!([
            { "name": "listed", "count": 2, "display_name": "Listed", "tags": ["popular"], "is_whitelisted": true },
            { "name": "unlisted", "count": 1, "is_whitelisted": false },
        ])
    );

    let without_reference = service(
        Arc::new(FakeHorde {
            models: models.clone(),
            ..FakeHorde::default()
        }),
        runtime,
    );
    assert_eq!(without_reference.text_models(false).await.unwrap(), models);
}
