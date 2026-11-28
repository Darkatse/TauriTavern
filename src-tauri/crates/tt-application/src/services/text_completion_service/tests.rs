use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use tt_contracts::observability::RequestDescriptionDto;
use tt_domain::errors::DomainError;
use tt_domain::ios_policy::{IosPolicyScope, resolve_ios_policy_activation_report};
use tt_domain::models::secret::Secrets;
use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
use tt_ports::repositories::secret_repository::SecretRepository;
use tt_ports::repositories::text_completion_repository::{
    TextCompletionEndpoint, TextCompletionRepository, TextCompletionStreamFormat,
};

use super::TextCompletionService;
use crate::dto::text_completion_dto::TextCompletionStatusRequestDto;
use crate::services::generation_runtime::GenerationRuntime;

#[derive(Default)]
struct FakeServer {
    reads: Mutex<Vec<TextCompletionEndpoint>>,
}

#[async_trait]
impl TextCompletionRepository for FakeServer {
    async fn get(
        &self,
        endpoint: &TextCompletionEndpoint,
        _path: &str,
    ) -> Result<Value, DomainError> {
        self.reads.lock().unwrap().push(endpoint.clone());
        Ok(json!({ "data": [] }))
    }

    async fn post(
        &self,
        _endpoint: &TextCompletionEndpoint,
        _path: &str,
        _body: &Value,
    ) -> Result<Value, DomainError> {
        unimplemented!()
    }

    async fn download(
        &self,
        _endpoint: &TextCompletionEndpoint,
        _path: &str,
        _body: &Value,
    ) -> Result<Value, DomainError> {
        unimplemented!()
    }

    async fn generate(
        &self,
        _endpoint: &TextCompletionEndpoint,
        _path: &str,
        _body: &Value,
    ) -> Result<Value, DomainError> {
        unimplemented!()
    }

    async fn generate_stream(
        &self,
        _endpoint: &TextCompletionEndpoint,
        _path: &str,
        _body: &Value,
        _format: TextCompletionStreamFormat,
        _sender: GenerationStreamSender,
        _cancel: GenerationCancelReceiver,
    ) -> Result<(), DomainError> {
        unimplemented!()
    }

    fn describe_request(
        &self,
        _endpoint: &TextCompletionEndpoint,
        _path: &str,
        _stream: Option<TextCompletionStreamFormat>,
    ) -> Result<RequestDescriptionDto, DomainError> {
        unimplemented!()
    }
}

struct EveryKeySet;

#[async_trait]
impl SecretRepository for EveryKeySet {
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
        Ok(Some("secret".to_string()))
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

#[tokio::test]
async fn only_servers_the_user_names_receive_the_page_address_and_need_consent() {
    let server = Arc::new(FakeServer::default());
    let policy = resolve_ios_policy_activation_report(IosPolicyScope::Ignored, None, None).unwrap();
    let service = TextCompletionService::new(
        server.clone(),
        Arc::new(EveryKeySet),
        Arc::new(GenerationRuntime::new(None, true)),
        policy,
    );
    let status = |api_type: &str| TextCompletionStatusRequestDto {
        api_type: api_type.to_string(),
        api_server: "https://proxy.example/v1".to_string(),
    };

    let cloud = service.prepare_status(status("featherless")).unwrap();
    assert_eq!(cloud.user_endpoint(), None);
    service.get_status(cloud).await.unwrap();
    let local = service.prepare_status(status("vllm")).unwrap();
    assert_eq!(
        local.user_endpoint().map(url::Url::as_str),
        Some("https://proxy.example/")
    );
    service.get_status(local).await.unwrap();

    let reads = server.reads.lock().unwrap();
    assert!(!reads[0].user_configured_endpoint);
    assert!(!reads[0].base_url.contains("proxy.example"));
    assert!(reads[1].user_configured_endpoint);
    assert!(reads.iter().all(|read| {
        read.headers
            .contains(&("Authorization".to_string(), "Bearer secret".to_string()))
    }));
}
