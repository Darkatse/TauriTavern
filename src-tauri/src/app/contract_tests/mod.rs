use std::collections::VecDeque;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde_json::{Value, json};
use tokio::fs;
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

use crate::application::dto::agent_dto::{
    AgentResolveChatCommitDto, AgentResolvePersistentStateMetadataUpdateDto,
};
use crate::application::dto::character_dto::{
    BulkMergeCharacterCardDataDto, BulkMergeCharacterCardDataFilterDto,
    CharacterLorebookConflictResolution, CheckCharacterLorebookConflictDto, CreateCharacterDto,
    ExportCharacterContentDto, ExportCharacterDto, ImportCharacterDto, MergeCharacterCardDataDto,
    ResolveCharacterLorebookConflictDto, UpdateAvatarDto, UpdateCharacterCardDataDto,
    UpdateCharacterDto,
};
use crate::application::dto::chat_completion_dto::ChatCompletionGenerateRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::{
    AgentModelExchange, AgentModelGateway, decode_chat_completion_response,
};
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService,
};
use crate::application::services::agent_runtime_service::AgentRuntimeService;
use crate::application::services::agent_tools::BuiltinAgentToolRegistry;
use crate::application::services::agent_workspace_lifecycle_service::{
    AgentRunActivity, AgentWorkspaceLifecycleService,
};
use crate::application::services::character_service::CharacterService;
use crate::application::services::llm_connection_service::LlmConnectionService;
use crate::application::services::skill_service::SkillService;
use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::{AgentDelegationPolicy, AgentProfileId};
use crate::domain::models::agent::{
    AgentChatRef, AgentModelContentPart, AgentModelRequest, AgentRun, AgentRunEventLevel,
    AgentRunPresentation, AgentRunStatus, WorkspacePath,
};
use crate::domain::models::preset::{DefaultPreset, Preset, PresetType};
use crate::infrastructure::persistence::png_utils::{
    read_character_data_from_png, write_character_data_to_png,
};
use crate::infrastructure::repositories::file_agent_profile_repository::FileAgentProfileRepository;
use crate::infrastructure::repositories::file_agent_repository::FileAgentRepository;
use crate::infrastructure::repositories::file_character_repository::FileCharacterRepository;
use crate::infrastructure::repositories::file_skill_repository::FileSkillRepository;
use crate::infrastructure::repositories::file_world_info_repository::FileWorldInfoRepository;
use tt_adapter_storage_core::FileChatRepository;
use tt_adapter_storage_core::FileLlmConnectionRepository;
use tt_adapter_storage_core::chat_directory_identity::new_shared_chat_alias_store_for_user_dir;
use tt_ports::repositories::agent_invocation_repository::AgentInvocationRepository;
use tt_ports::repositories::agent_profile_repository::AgentProfileRepository;
use tt_ports::repositories::agent_profile_storage_health_repository::AgentProfileStorageHealthRepository;
use tt_ports::repositories::agent_run_repository::{AgentRunEventReadQuery, AgentRunRepository};
use tt_ports::repositories::agent_workspace_lifecycle_repository::AgentWorkspaceLifecycleRepository;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::checkpoint_repository::CheckpointRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::repositories::preset_repository::PresetRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::repositories::world_info_repository::WorldInfoRepository;

const AGENT_CONTRACT_ASYNC_TIMEOUT: Duration = Duration::from_secs(5);

mod agent_runtime;
mod architecture;
mod character;
mod host_resources;

struct AgentRuntimeFixture {
    service: Arc<AgentRuntimeService>,
    agent_repository: Arc<FileAgentRepository>,
    profile_service: Arc<AgentProfileService>,
    model_gateway: Arc<MockAgentModelGateway>,
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tauritavern-contract-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

async fn character_service(root: &Path) -> CharacterService {
    character_service_with_world_repository(root).await.0
}

async fn character_service_with_world_repository(
    root: &Path,
) -> (CharacterService, Arc<FileWorldInfoRepository>) {
    let default_user = root.join("default-user");
    let characters = default_user.join("characters");
    let chats = default_user.join("chats");
    let thumbnails = default_user.join("thumbnails/avatar");
    let default_avatar = default_user.join("default.png");
    fs::create_dir_all(&characters)
        .await
        .expect("create characters dir");
    fs::create_dir_all(&chats).await.expect("create chats dir");
    fs::create_dir_all(&thumbnails)
        .await
        .expect("create thumbnails dir");
    fs::write(&default_avatar, minimal_png())
        .await
        .expect("write default avatar");

    let aliases = new_shared_chat_alias_store_for_user_dir(&default_user);
    let character_repository = Arc::new(FileCharacterRepository::with_chat_aliases(
        characters.clone(),
        chats.clone(),
        thumbnails,
        default_avatar,
        aliases.clone(),
    ));
    let chat_repository: Arc<dyn ChatRepository> = Arc::new(FileChatRepository::with_chat_aliases(
        characters,
        chats,
        default_user.join("group chats"),
        default_user.join("backups"),
        aliases,
    ));
    let world_repository = Arc::new(FileWorldInfoRepository::new(default_user.join("worlds")));
    let agent_repository = Arc::new(FileAgentRepository::new(
        root.join("_tauritavern/agent-workspaces"),
    ));
    let lifecycle_repository: Arc<dyn AgentWorkspaceLifecycleRepository> = agent_repository;
    let lifecycle_service = Arc::new(AgentWorkspaceLifecycleService::new(
        lifecycle_repository,
        Arc::new(NoActiveAgentRuns),
    ));

    (
        CharacterService::new(
            character_repository,
            chat_repository,
            world_repository.clone(),
            lifecycle_service,
        ),
        world_repository,
    )
}

fn agent_runtime_fixture(root: &Path) -> AgentRuntimeFixture {
    agent_runtime_fixture_with_responses(root, default_agent_responses())
}

fn agent_runtime_fixture_with_responses(root: &Path, responses: Vec<Value>) -> AgentRuntimeFixture {
    agent_runtime_fixture_with_results(root, responses.into_iter().map(Ok).collect())
}

fn agent_runtime_fixture_with_results(
    root: &Path,
    responses: Vec<Result<Value, ApplicationError>>,
) -> AgentRuntimeFixture {
    let default_user = root.join("default-user");
    let aliases = new_shared_chat_alias_store_for_user_dir(&default_user);
    let agent_repository = Arc::new(FileAgentRepository::new(
        root.join("_tauritavern/agent-workspaces"),
    ));
    let chat_file_repository = Arc::new(FileChatRepository::with_chat_aliases(
        default_user.join("characters"),
        default_user.join("chats"),
        default_user.join("group chats"),
        default_user.join("backups"),
        aliases,
    ));
    let profile_file_repository = Arc::new(FileAgentProfileRepository::new(
        root.join("_tauritavern/agent-profiles"),
    ));
    let profile_repository: Arc<dyn AgentProfileRepository> = profile_file_repository.clone();
    let profile_health_repository: Arc<dyn AgentProfileStorageHealthRepository> =
        profile_file_repository;
    let profile_service = Arc::new(AgentProfileService::new(
        profile_repository,
        profile_health_repository,
        Arc::new(NullPresetRepository),
    ));
    let skill_service = Arc::new(SkillService::new(Arc::new(FileSkillRepository::new(
        root.join("_tauritavern/skills"),
    ))));
    let llm_connection_service = Arc::new(LlmConnectionService::new(Arc::new(
        FileLlmConnectionRepository::new(root.join("_tauritavern/llm-connections")),
    )));
    let model_gateway = Arc::new(MockAgentModelGateway::with_results(responses));
    let service = Arc::new(AgentRuntimeService::new(
        agent_repository.clone() as Arc<dyn AgentRunRepository>,
        agent_repository.clone() as Arc<dyn AgentInvocationRepository>,
        agent_repository.clone() as Arc<dyn WorkspaceRepository>,
        agent_repository.clone() as Arc<dyn CheckpointRepository>,
        chat_file_repository.clone() as Arc<dyn ChatRepository>,
        chat_file_repository as Arc<dyn GroupChatRepository>,
        skill_service,
        model_gateway.clone(),
        profile_service.clone(),
        llm_connection_service,
    ));

    AgentRuntimeFixture {
        service,
        agent_repository,
        profile_service,
        model_gateway,
    }
}

fn default_agent_responses() -> Vec<Value> {
    vec![
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "I will write the artifact.",
                    "tool_calls": [{
                        "id": "call_write",
                        "type": "function",
                        "function": {
                            "name": "workspace_write_file",
                            "arguments": "{\"path\":\"output/main.md\",\"content\":\"hello from real repo\"}"
                        }
                    }]
                }
            }]
        }),
        json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_finish",
                        "type": "function",
                        "function": {
                            "name": "workspace_finish",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        }),
    ]
}

async fn resolve_contract_profile(
    fixture: &AgentRuntimeFixture,
) -> crate::domain::models::agent::profile::ResolvedAgentProfile {
    let registry = BuiltinAgentToolRegistry::phase2c();
    fixture
        .profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: None,
            known_tools: registry.specs(),
        })
        .await
        .expect("resolve default profile")
}

fn contract_run(
    id: &str,
    presentation: AgentRunPresentation,
    profile: &crate::domain::models::agent::profile::ResolvedAgentProfile,
) -> AgentRun {
    AgentRun {
        id: id.to_string(),
        workspace_id: format!("{id}_workspace"),
        stable_chat_id: format!("{id}_stable_chat"),
        chat_ref: AgentChatRef::Character {
            character_id: "Alice".to_string(),
            file_name: "Alice.png".to_string(),
        },
        generation_type: "normal".to_string(),
        profile_id: Some(profile.id.as_str().to_string()),
        skill_scope_refs: Default::default(),
        persist_base_state_id: None,
        input_message_count: None,
        presentation,
        status: AgentRunStatus::Created,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn create_character(name: &str, json_data: Option<Value>) -> CreateCharacterDto {
    CreateCharacterDto {
        file_name: Some(name.to_string()),
        json_data: json_data.map(|value| serde_json::to_string(&value).unwrap()),
        primary_lorebook: None,
        name: name.to_string(),
        description: "description".to_string(),
        personality: "personality".to_string(),
        scenario: "scenario".to_string(),
        first_mes: "hello".to_string(),
        mes_example: String::new(),
        creator: None,
        creator_notes: None,
        character_version: None,
        tags: None,
        talkativeness: None,
        fav: None,
        alternate_greetings: None,
        system_prompt: None,
        post_history_instructions: None,
        extensions: None,
    }
}

fn character_card(name: &str, extensions: Value) -> Value {
    json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "name": name,
        "description": "description",
        "personality": "personality",
        "scenario": "scenario",
        "first_mes": "hello",
        "mes_example": "",
        "data": {
            "name": name,
            "description": "description",
            "personality": "personality",
            "scenario": "scenario",
            "first_mes": "hello",
            "mes_example": "",
            "creator": "",
            "creator_notes": "",
            "character_version": "",
            "alternate_greetings": [],
            "tags": [],
            "extensions": extensions,
        },
        "unknownTop": {
            "kept": true
        }
    })
}

fn empty_update_character() -> UpdateCharacterDto {
    UpdateCharacterDto {
        name: None,
        chat: None,
        description: None,
        personality: None,
        scenario: None,
        first_mes: None,
        mes_example: None,
        creator: None,
        creator_notes: None,
        character_version: None,
        tags: None,
        talkativeness: None,
        fav: None,
        alternate_greetings: None,
        system_prompt: None,
        post_history_instructions: None,
        extensions: None,
    }
}

fn world_info(content: &str) -> Value {
    json!({
        "entries": {
            "1": {
                "uid": 1,
                "key": ["alpha"],
                "comment": "memo",
                "content": content,
                "order": 0,
                "position": 0,
                "disable": false
            }
        }
    })
}

fn character_book(name: &str, content: &str) -> Value {
    json!({
        "name": name,
        "entries": [{
            "uid": 1,
            "key": ["alpha"],
            "content": content,
            "extensions": {}
        }],
        "extensions": {}
    })
}

fn character_png(card: &Value) -> Vec<u8> {
    write_character_data_to_png(
        &minimal_png(),
        &serde_json::to_string(card).expect("serialize character card"),
    )
    .expect("write character card to png")
}

async fn read_stored_card(root: &Path, name: &str) -> Value {
    let stored_png = fs::read(root.join(format!("default-user/characters/{name}.png")))
        .await
        .expect("read stored character png");
    read_card_json(&stored_png)
}

fn read_card_json(png: &[u8]) -> Value {
    let card_json = read_character_data_from_png(png).expect("read character card from png");
    serde_json::from_str(&card_json).expect("parse character card json")
}

async fn execute_agent_loop_with_host_resolver<R>(
    service: Arc<AgentRuntimeService>,
    run_id: String,
    prompt_snapshot: Value,
    request: ChatCompletionGenerateRequestDto,
    profile: crate::domain::models::agent::profile::ResolvedAgentProfile,
    cancel_receiver: &mut watch::Receiver<bool>,
    resolver: R,
) -> Result<(), ApplicationError>
where
    R: std::future::Future<Output = Result<(), ApplicationError>>,
{
    let (loop_result, resolver_result) =
        tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
            tokio::join!(
                service.execute_agent_loop_run_inner(
                    &run_id,
                    prompt_snapshot,
                    request,
                    profile,
                    cancel_receiver,
                ),
                resolver,
            )
        })
        .await
        .expect("agent loop and host resolver timed out");
    resolver_result.expect("host resolver");
    loop_result
}

async fn resolve_next_chat_commit_and_persistent_state_update(
    service: Arc<AgentRuntimeService>,
    repository: Arc<FileAgentRepository>,
    run_id: String,
    message_id: &'static str,
) -> Result<(), ApplicationError> {
    let commit_id =
        wait_for_event_field(&repository, &run_id, "chat_commit_requested", "commitId").await?;
    service
        .resolve_chat_commit(AgentResolveChatCommitDto {
            run_id: run_id.clone(),
            commit_id,
            message_id: Some(message_id.to_string()),
            error: None,
        })
        .await?;

    let update_id = wait_for_event_field(
        &repository,
        &run_id,
        "persistent_state_metadata_update_requested",
        "updateId",
    )
    .await?;
    service
        .resolve_persistent_state_metadata_update(AgentResolvePersistentStateMetadataUpdateDto {
            run_id,
            update_id,
            error: None,
        })
        .await
}

async fn wait_for_event_field(
    repository: &FileAgentRepository,
    run_id: &str,
    event_type: &str,
    field: &str,
) -> Result<String, ApplicationError> {
    tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
        loop {
            let events = read_agent_events(repository, run_id).await;
            if let Some(value) = events
                .iter()
                .find(|event| event.event_type == event_type)
                .and_then(|event| event.payload[field].as_str())
            {
                return Ok(value.to_string());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| ApplicationError::InternalError(format!("{event_type}.{field} timed out")))?
}

async fn read_agent_events(
    repository: &FileAgentRepository,
    run_id: &str,
) -> Vec<crate::domain::models::agent::AgentRunEvent> {
    repository
        .read_events(
            run_id,
            AgentRunEventReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: 300,
                invocation_id: None,
            },
        )
        .await
        .expect("read events")
}

async fn read_workspace_json(repository: &FileAgentRepository, run_id: &str, path: &str) -> Value {
    let file = repository
        .read_text(run_id, &WorkspacePath::parse(path).expect("workspace path"))
        .await
        .expect("read workspace json");
    serde_json::from_str(&file.text).expect("parse workspace json")
}

fn tool_result_structured_values(request: &AgentModelRequest, name: &str) -> Vec<Value> {
    request
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            AgentModelContentPart::ToolResult { result } if result.name == name => {
                Some(result.structured.clone())
            }
            _ => None,
        })
        .collect()
}

async fn wait_for_closed_sessions(gateway: &MockAgentModelGateway, expected: Vec<String>) {
    let mut expected = expected;
    expected.sort();
    tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, async {
        loop {
            let mut sessions = gateway.closed_sessions().await;
            sessions.sort();
            if sessions == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("model sessions were not closed");
}

fn chat_request(user_content: &str) -> ChatCompletionGenerateRequestDto {
    ChatCompletionGenerateRequestDto {
        payload: json!({
            "chat_completion_source": "openai",
            "model": "test-model",
            "messages": [{
                "role": "user",
                "content": user_content
            }]
        })
        .as_object()
        .cloned()
        .unwrap(),
    }
}

fn minimal_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::new(1, 1));
    let mut output = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut output), ImageFormat::Png)
        .expect("build minimal png");
    output
}

struct NoActiveAgentRuns;

#[async_trait]
impl AgentRunActivity for NoActiveAgentRuns {
    async fn active_run_ids(&self) -> Result<Vec<String>, ApplicationError> {
        Ok(Vec::new())
    }

    async fn active_run_ids_for_workspace(
        &self,
        _workspace_id: &str,
    ) -> Result<Vec<String>, ApplicationError> {
        Ok(Vec::new())
    }
}

struct NullPresetRepository;

#[async_trait]
impl PresetRepository for NullPresetRepository {
    async fn save_preset(&self, _preset: &Preset) -> Result<(), DomainError> {
        Ok(())
    }

    async fn delete_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<(), DomainError> {
        Ok(())
    }

    async fn preset_exists(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<bool, DomainError> {
        Ok(false)
    }

    async fn get_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<Option<Preset>, DomainError> {
        Ok(None)
    }

    async fn list_presets(&self, _preset_type: &PresetType) -> Result<Vec<String>, DomainError> {
        Ok(Vec::new())
    }

    async fn get_default_preset(
        &self,
        _name: &str,
        _preset_type: &PresetType,
    ) -> Result<Option<DefaultPreset>, DomainError> {
        Ok(None)
    }
}

struct MockAgentModelGateway {
    responses: Mutex<VecDeque<Result<Value, ApplicationError>>>,
    requests: Mutex<Vec<AgentModelRequest>>,
    closed_sessions: Mutex<Vec<String>>,
}

impl MockAgentModelGateway {
    fn with_results(responses: Vec<Result<Value, ApplicationError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            closed_sessions: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<AgentModelRequest> {
        self.requests.lock().await.clone()
    }

    async fn closed_sessions(&self) -> Vec<String> {
        self.closed_sessions.lock().await.clone()
    }
}

#[async_trait]
impl AgentModelGateway for MockAgentModelGateway {
    async fn generate_with_cancel(
        &self,
        request: AgentModelRequest,
        _cancel: watch::Receiver<bool>,
    ) -> Result<AgentModelExchange, ApplicationError> {
        self.requests.lock().await.push(request.clone());
        let response = self.responses.lock().await.pop_front().ok_or_else(|| {
            ApplicationError::ValidationError(
                "mock_model.empty_responses: no response left".to_string(),
            )
        })??;
        let response = decode_chat_completion_response(response, &request.tools)?;
        Ok(AgentModelExchange {
            response,
            provider_state: request.provider_state,
        })
    }

    async fn close_session(&self, session_id: &str) {
        self.closed_sessions
            .lock()
            .await
            .push(session_id.to_string());
    }
}
