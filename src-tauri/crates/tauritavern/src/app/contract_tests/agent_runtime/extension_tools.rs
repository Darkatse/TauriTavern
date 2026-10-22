use super::*;

use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::mpsc;
use tt_application::dto::agent_dto::{AgentCancelRunDto, AgentReadSessionDto, AgentSaveProfileDto};
use tt_contracts::extension_tools::{ExtensionToolDefinition, ExtensionToolReply};
use tt_domain::models::tool::{AgentToolScope, ToolId};

use super::session::{
    configure_session_profile, prepare_input, user_input, wait_until_session_idle,
};

fn register_tool(
    fixture: &AgentRuntimeFixture,
    extension_id: &str,
    name: &str,
    contexts: Vec<AgentToolScope>,
) -> (ToolId, mpsc::UnboundedReceiver<Value>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let channel = Channel::new(move |body| {
        let InvokeResponseBody::Json(body) = body else {
            panic!("expected JSON tool event")
        };
        sender.send(serde_json::from_str(&body).unwrap()).unwrap();
        Ok(())
    });
    let id = fixture.extension_tools.register("main", ExtensionToolDefinition {
                returns_images: false,
        extension_id: extension_id.into(),
        name: name.into(),
        description: "Inspect application state".into(),
        input_schema: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        contexts,
        enabled: true,
    }, channel).unwrap();
    (id, receiver)
}

async fn next_event(events: &mut mpsc::UnboundedReceiver<Value>) -> Value {
    tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, events.recv())
        .await
        .expect("extension tool event timed out")
        .expect("extension channel closed")
}

#[tokio::test]
async fn chat_routes_colliding_extension_names_without_replacing_builtin_tools() {
    let root = temp_root("chat-extension-tool");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call("inspect", "inspect", json!({"query":"theme"})),
                model_tool_call("inspect_other", "inspect__2", json!({"query":"layout"})),
                model_tool_call(
                    "extension_write",
                    "workspace_write_file__2",
                    json!({"query":"extension state"}),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call(
                    "write",
                    "workspace_write_file",
                    json!({"path":"output/main.md","content":"Theme inspected"}),
                ),
                model_tool_call("finish", "workspace_finish", json!({})),
            ]),
        ],
    );
    let (tool_id, mut events) = register_tool(
        &fixture,
        "contract",
        "inspect",
        vec![AgentToolScope::Chat, AgentToolScope::Session],
    );
    let (other_id, mut other_events) =
        register_tool(&fixture, "another", "inspect", vec![AgentToolScope::Chat]);
    let (collision_id, mut collision_events) = register_tool(
        &fixture,
        "contract",
        "workspace_write_file",
        vec![AgentToolScope::Chat],
    );
    let mut definition = fixture
        .profile_service
        .load_profile("default-writer")
        .await
        .unwrap()
        .unwrap();
    definition.id = AgentProfileId::parse("extension-writer").unwrap();
    definition.tools.allow.splice(
        0..0,
        [
            tool_id.to_string(),
            other_id.to_string(),
            collision_id.to_string(),
        ],
    );
    fixture.service.save_profile(definition).await.unwrap();
    let profile = fixture
        .profile_service
        .resolve_profile(AgentProfileResolveInput {
            profile_id: Some("extension-writer"),
            tool_catalog: fixture.service.tool_catalog(),
        })
        .await
        .unwrap();
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "extension-chat",
        Some(false),
    )
    .await;
    let ordinary_json = json!({
        "theme": "dark",
        "structuredContent": { "mode": "ordinary business data" },
        "diagnostics": [{ "message": "This is returned data, not protocol metadata" }],
    });
    for (events, id, query, value) in [
        (&mut events, &tool_id, "theme", ordinary_json.clone()),
        (
            &mut other_events,
            &other_id,
            "layout",
            json!({"layout":"wide"}),
        ),
        (
            &mut collision_events,
            &collision_id,
            "extension state",
            json!({"ready":true}),
        ),
    ] {
        let event = next_event(events).await;
        assert_eq!(event["call"]["runId"], handle.run_id);
        assert_eq!(event["call"]["target"]["kind"], "chat");
        assert_eq!(event["call"]["toolId"], id.as_str());
        assert_eq!(event["call"]["arguments"], json!({"query":query}));
        assert!(fixture.extension_tools.resolve(
            "main",
            event["requestId"].as_str().unwrap(),
            ExtensionToolReply::Value {
                value,
                images: vec![]
            },
        ));
    }
    wait_until_session_idle(&fixture, &handle.run_id).await;
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&handle.run_id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Completed
    );
    let requests = fixture.model_gateway.requests().await;
    for (call_id, id, value) in [
        ("inspect", tool_id, ordinary_json),
        ("inspect_other", other_id, json!({"layout":"wide"})),
        ("extension_write", collision_id, json!({"ready":true})),
    ] {
        let result = requests[1]
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .find_map(|part| match part {
                AgentModelContentPart::ToolResult { result } if result.call_id == call_id => {
                    Some(result)
                }
                _ => None,
            })
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.tool_id, id);
        assert_eq!(
            serde_json::from_str::<Value>(&result.content).unwrap(),
            value
        );
    }
    let files = fixture
        .agent_repository
        .open_filesystem(&handle.run_id)
        .await
        .unwrap();
    assert_eq!(
        files
            .read_text(&WorkspacePath::parse("output/main.md").unwrap())
            .await
            .unwrap()
            .text,
        "Theme inspected"
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn session_filters_tools_and_preserves_profile_choices_when_extensions_are_unavailable() {
    let root = temp_root("session-extension-tools");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            json!({"choices":[{"message":{"role":"assistant","content":"The tool is disabled."}}]}),
            model_tool_response(vec![model_tool_call("inspect", "inspect", json!({}))]),
            json!({"choices":[{"message":{"role":"assistant","content":"The inspection reported an error."}}]}),
            json!({"choices":[{"message":{"role":"assistant","content":"Continuing without extensions."}}]}),
        ],
    );
    let mut profile = configure_session_profile(&fixture, &root).await;
    let tool_id = ToolId::extension("contract", "inspect").unwrap();
    profile.tools.allow.push(tool_id.to_string());
    let error = fixture
        .service
        .save_session_profile(AgentSaveProfileDto {
            profile: profile.clone(),
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("extension.tool_unavailable"));

    let (_, mut events) = register_tool(
        &fixture,
        "contract",
        "inspect",
        vec![AgentToolScope::Session],
    );
    let (chat_only, _chat_events) = register_tool(
        &fixture,
        "contract",
        "chat_only",
        vec![AgentToolScope::Chat],
    );
    profile.tools.allow.push(chat_only.to_string());
    fixture
        .service
        .save_session_profile(AgentSaveProfileDto {
            profile: profile.clone(),
        })
        .await
        .unwrap();
    let session = fixture.service.create_session().await.unwrap().session;

    fixture
        .extension_tools
        .set_enabled("main", &tool_id, false)
        .unwrap();
    let input = prepare_input(
        &fixture,
        &session.id,
        &profile,
        user_input("Continue while disabled."),
    )
    .await;
    let disabled = fixture.service.start_session_run(input).await.unwrap();
    wait_until_session_idle(&fixture, &disabled.run_id).await;
    let requests = fixture.model_gateway.requests().await;
    assert!(
        requests[0]
            .tools
            .iter()
            .all(|tool| tool.tool_id.extension_id().is_none())
    );

    fixture
        .extension_tools
        .set_enabled("main", &tool_id, true)
        .unwrap();
    let input = prepare_input(
        &fixture,
        &session.id,
        &profile,
        user_input("Inspect the application."),
    )
    .await;
    let enabled = fixture.service.start_session_run(input).await.unwrap();
    let event = next_event(&mut events).await;
    assert_eq!(
        event["call"]["target"],
        json!({"kind":"session","sessionId":session.id})
    );
    assert!(fixture.extension_tools.resolve(
        "main",
        event["requestId"].as_str().unwrap(),
        ExtensionToolReply::Error {
            message: "Inspection failed in the extension".into(),
        }
    ));
    wait_until_session_idle(&fixture, &enabled.run_id).await;
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&enabled.run_id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Completed
    );
    let requests = fixture.model_gateway.requests().await;
    assert!(requests[1].tools.iter().any(|tool| tool.tool_id == tool_id));
    assert!(
        requests[1]
            .tools
            .iter()
            .all(|tool| tool.tool_id != chat_only)
    );
    let history = fixture
        .service
        .read_session(AgentReadSessionDto {
            session_id: session.id.clone(),
            before_seq: None,
            limit: Some(100),
        })
        .await
        .unwrap();
    assert!(history.messages.iter().flat_map(|entry| &entry.message.parts).any(|part| matches!(part,
        AgentModelContentPart::ToolResult { result } if result.is_error && result.content.contains("Inspection failed in the extension")
    )));

    fixture.extension_tools.clear_page("main");
    profile.display_name = "Edited with the extension unloaded".into();
    fixture
        .service
        .save_session_profile(AgentSaveProfileDto {
            profile: profile.clone(),
        })
        .await
        .unwrap();
    let stored = fixture
        .service
        .load_session_profile()
        .await
        .unwrap()
        .profile
        .unwrap();
    assert!(stored.tools.allow.contains(&tool_id.to_string()));
    let input = prepare_input(
        &fixture,
        &session.id,
        &stored,
        user_input("Continue after reload."),
    )
    .await;
    let missing = fixture.service.start_session_run(input).await.unwrap();
    wait_until_session_idle(&fixture, &missing.run_id).await;
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&missing.run_id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Completed
    );
    assert!(
        fixture.model_gateway.requests().await[3]
            .tools
            .iter()
            .all(|tool| tool.tool_id.extension_id().is_none())
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn session_cancellation_stops_after_the_started_extension_call() {
    let root = temp_root("session-extension-cancel");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![model_tool_response(vec![
            model_tool_call("waiting", "inspect", json!({})),
            model_tool_call(
                "after_cancel",
                "workspace_write_file",
                json!({"path":"work/must-not-exist.md","content":"must not run"}),
            ),
        ])],
    );
    let (tool_id, mut events) = register_tool(
        &fixture,
        "contract",
        "inspect",
        vec![AgentToolScope::Session],
    );
    let mut profile = configure_session_profile(&fixture, &root).await;
    profile.tools.allow.push(tool_id.to_string());
    fixture
        .service
        .save_session_profile(AgentSaveProfileDto {
            profile: profile.clone(),
        })
        .await
        .unwrap();
    let session = fixture.service.create_session().await.unwrap().session;
    let input = prepare_input(
        &fixture,
        &session.id,
        &profile,
        user_input("Wait for cancellation."),
    )
    .await;
    let handle = fixture.service.start_session_run(input).await.unwrap();
    let event = next_event(&mut events).await;
    fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        next_event(&mut events).await,
        json!({"type":"cancel","requestId":event["requestId"]})
    );
    wait_until_session_idle(&fixture, &handle.run_id).await;
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&handle.run_id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Cancelled
    );
    assert!(!fixture.extension_tools.resolve(
        "main",
        event["requestId"].as_str().unwrap(),
        ExtensionToolReply::Value {
            value: Value::Null,
            images: vec![]
        }
    ));
    let run_events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    assert!(
        run_events
            .iter()
            .all(|event| event.payload["callId"] != "after_cancel")
    );
    assert_eq!(fixture.model_gateway.requests().await.len(), 1);
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn tool_images_are_normalized_stored_as_references_and_hydrated_only_for_the_model() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use tt_contracts::extension_tools::ExtensionToolImage;

    for accepts_images in [true, false] {
        let root = temp_root("session-tool-images");
        let fixture = agent_runtime_fixture_with_responses(
            &root,
            vec![
                model_tool_response(vec![model_tool_call("capture", "inspect", json!({}))]),
                model_tool_response(vec![model_tool_call(
                    "list",
                    "workspace_list_files",
                    json!({}),
                )]),
                json!({"choices":[{"message":{"role":"assistant","content":"Inspected."}}]}),
            ],
        );
        let (tool_id, mut events) = register_tool(
            &fixture,
            "contract",
            "inspect",
            vec![AgentToolScope::Session],
        );
        let mut profile = configure_session_profile(&fixture, &root).await;
        profile.tools.allow = vec![tool_id.to_string(), "builtin:workspace.list_files".into()];
        let session = fixture.service.create_session().await.unwrap().session;
        let mut input = prepare_input(
            &fixture,
            &session.id,
            &profile,
            user_input("Inspect the page."),
        )
        .await;
        input.prompt_snapshot["acceptsImages"] = json!(accepts_images);
        let handle = fixture.service.start_session_run(input).await.unwrap();
        let event = next_event(&mut events).await;
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2000,
            1000,
            image::Rgb([24, 80, 160]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        assert!(fixture.extension_tools.resolve(
            "main",
            event["requestId"].as_str().unwrap(),
            ExtensionToolReply::Value {
                value: json!("Page captured."),
                images: vec![
                    ExtensionToolImage {
                        data: "invalid base64!".into()
                    },
                    ExtensionToolImage {
                        data: STANDARD.encode(png.into_inner())
                    },
                    ExtensionToolImage {
                        data: STANDARD.encode(b"not an image")
                    }
                ],
            }
        ));
        wait_until_session_idle(&fixture, &handle.run_id).await;
        assert_eq!(
            fixture
                .agent_repository
                .load_run(&handle.run_id)
                .await
                .unwrap()
                .status,
            AgentRunStatus::Completed
        );
        let history = fixture
            .service
            .read_session(AgentReadSessionDto {
                session_id: session.id,
                before_seq: None,
                limit: Some(100),
            })
            .await
            .unwrap();
        let result = history
            .messages
            .iter()
            .flat_map(|entry| &entry.message.parts)
            .find_map(|part| match part {
                AgentModelContentPart::ToolResult { result } if result.call_id == "capture" => {
                    Some(result)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(result.images.len(), 1);
        assert_eq!(result.images[0].mime_type, "image/jpeg");
        assert!(result.content.starts_with("Page captured."));
        assert_eq!(result.content.matches("could not be decoded").count(), 2);
        assert!(
            result
                .content
                .ends_with("[The tool returned an image that could not be decoded.]")
        );
        assert!(
            history
                .messages
                .iter()
                .flat_map(|entry| &entry.message.parts)
                .all(|part| !matches!(part, AgentModelContentPart::Media { .. }))
        );
        let files = fixture
            .agent_repository
            .open_filesystem(&handle.run_id)
            .await
            .unwrap();
        let stored_files = files.list_files(None, 8, 100).await.unwrap().entries;
        let image_files = stored_files
            .iter()
            .filter(|entry| {
                result
                    .images
                    .iter()
                    .any(|image| image.path == entry.path.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(image_files.len(), 1);
        assert_eq!(image_files[0].path.as_str(), result.images[0].path);
        let bytes = files
            .read_file(
                &image_files[0].path,
                tt_ports::tool_images::MAX_TOOL_IMAGE_BYTES,
            )
            .await
            .unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (1280, 640));
        let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
        let stored = events
            .iter()
            .find(|event| {
                event.event_type == "tool_result_stored" && event.payload["callId"] == "capture"
            })
            .unwrap();
        let audit_path = WorkspacePath::parse(stored.payload["path"].as_str().unwrap()).unwrap();
        let audit: Value =
            serde_json::from_str(&files.read_text(&audit_path).await.unwrap().text).unwrap();
        assert_eq!(
            audit["images"],
            serde_json::to_value(&result.images).unwrap()
        );
        assert_eq!(audit["structured"], json!("Page captured."));
        assert_eq!(audit["content"], result.content);
        let requests = fixture.model_gateway.requests().await;
        assert_eq!(requests.len(), 3);
        let mut previous_media = None;
        for request in &requests[1..] {
            let message = request.messages.iter().find(|message| message.parts.iter().any(|part| matches!(part, AgentModelContentPart::ToolResult { result } if result.call_id == "capture"))).unwrap();
            let media = message
                .parts
                .iter()
                .filter(|part| matches!(part, AgentModelContentPart::Media { .. }))
                .collect::<Vec<_>>();
            if accepts_images {
                assert_eq!(media.len(), 1);
                if let Some(previous) = previous_media {
                    assert_eq!(previous, media[0]);
                }
                previous_media = Some(media[0]);
                let AgentModelContentPart::Media { mime_type, value } = media[0] else {
                    unreachable!()
                };
                assert_eq!(mime_type, "image/jpeg");
                assert_eq!(
                    value["image_url"]["url"],
                    format!("data:image/jpeg;base64,{}", STANDARD.encode(&bytes))
                );
            } else {
                assert!(media.is_empty());
                assert!(message.parts.iter().any(|part| matches!(part, AgentModelContentPart::ToolResult { result } if result.content.ends_with("[The image is not shown: this model does not accept images.]"))));
            }
        }
        fs::remove_dir_all(root).await.unwrap();
    }
}
