use super::*;
use tt_application::dto::agent_dto::{AgentCancelRunDto, AgentResumeRunDto};
use tt_ports::workspace_fs::{WorkspaceWriteGuard, sha256_hex};
use tt_ports::workspace_shell::{
    WorkspaceShell, WorkspaceShellExit, WorkspaceShellRequest, WorkspaceShellResult,
};

#[tokio::test]
async fn shell_edits_share_cas_and_publish_the_last_text_file_each_round() {
    let root = temp_root("agent-shell-writing");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "hidden_read",
                    "workspace_shell",
                    json!({"command": "cp /input/prompt_snapshot.json /scratch/leaked.json"}),
                ),
                model_tool_call(
                    "readonly_write",
                    "workspace_shell",
                    json!({"command": r#"python3 -c 'from pathlib import Path; Path("/tool-results/forbidden.txt").write_text("forbidden")'"#}),
                ),
                model_tool_call(
                    "prepare",
                    "workspace_shell",
                    json!({
                        "command": concat!(
                            "mkdir -p scratch/review\n",
                            "printf '{\"text\":\"shell draft\"}' > scratch/review/input.json\n",
                            "jq -r .text scratch/review/input.json > scratch/review/draft.txt\n",
                            "cp scratch/review/draft.txt output/main.md\n",
                            "mv scratch/review/draft.txt output/Moved.MD\n",
                            "rm scratch/review/input.json\n",
                            "printf '{\"done\":true}' > scratch/review/state.json\n",
                            "cat output/main.md\n",
                        ),
                    }),
                ),
                model_tool_call(
                    "write_metadata",
                    "workspace_write_file",
                    json!({"path": "scratch/metadata.json", "content": "{}"}),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call(
                    "read_before_shell",
                    "workspace_read_file",
                    json!({"path": "output/main.md"}),
                ),
                model_tool_call(
                    "append",
                    "workspace_shell",
                    json!({
                        "command": r#"python3 -c 'with open("main.md", "a") as file: file.write("updated by python\n")'"#,
                        "workdir": "/output",
                    }),
                ),
                model_tool_call(
                    "stale_patch",
                    "workspace_apply_patch",
                    json!({
                        "path": "output/main.md",
                        "old_string": "shell draft",
                        "new_string": "stale replacement",
                    }),
                ),
                model_tool_call(
                    "read_after_shell",
                    "workspace_read_file",
                    json!({"path": "output/main.md"}),
                ),
                model_tool_call(
                    "fresh_patch",
                    "workspace_apply_patch",
                    json!({
                        "path": "output/main.md",
                        "old_string": "shell draft",
                        "new_string": "final draft",
                    }),
                ),
                model_tool_call(
                    "copy_final",
                    "workspace_shell",
                    json!({"command": r#"python3 -c 'from pathlib import Path; Path("scratch/review/final.md").write_text(Path("output/main.md").read_text())'"#}),
                ),
                model_tool_call(
                    "move_final",
                    "workspace_shell",
                    json!({"command": "mv scratch/review scratch/finalized"}),
                ),
                model_tool_call(
                    "inspect_final",
                    "workspace_shell",
                    json!({"command": "cat scratch/finalized/final.md"}),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call(
                    "commit",
                    "workspace_commit",
                    json!({"path": "scratch/finalized/final.md"}),
                ),
                model_tool_call("finish", "workspace_finish", json!({})),
            ]),
        ],
    );
    let profile = resolve_contract_profile(&fixture).await;
    let run = contract_run("shell_writing", AgentRunPresentation::Foreground, &profile);
    fixture.agent_repository.create_run(&run).await.unwrap();
    let request = chat_request("prepare and revise a draft with shell and text tools");
    let prompt_snapshot = json!({"chatCompletionPayload": request.payload.clone()});
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    execute_agent_loop_with_host_resolver(
        fixture.service.clone(),
        run.id.clone(),
        prompt_snapshot,
        request,
        profile,
        &mut cancel_receiver,
        resolve_chat_commits_and_persistent_state_update(
            fixture.service.clone(),
            fixture.agent_repository.clone(),
            run.id.clone(),
            "message_shell",
            &[],
        ),
    )
    .await
    .unwrap();

    let files = fixture
        .agent_repository
        .open_filesystem(&run.id)
        .await
        .unwrap();
    let final_text = "final draft\nupdated by python\n";
    assert_eq!(
        files
            .read_text(&WorkspacePath::parse("scratch/finalized/final.md").unwrap())
            .await
            .unwrap()
            .text,
        final_text,
    );
    for removed in [
        "scratch/finalized/draft.txt",
        "scratch/finalized/input.json",
        "scratch/leaked.json",
        "tool-results/forbidden.txt",
    ] {
        assert!(matches!(
            files
                .metadata(Some(&WorkspacePath::parse(removed).unwrap()))
                .await,
            Err(DomainError::NotFound(_)),
        ));
    }
    let events = read_agent_events(&fixture.agent_repository, &run.id).await;
    for denied in ["hidden_read", "readonly_write"] {
        assert!(events.iter().any(|event| {
            event.event_type == "tool_call_failed" && event.payload["callId"] == denied
        }));
    }
    assert!(events.iter().any(|event| {
        event.event_type == "tool_call_failed"
            && event.payload["callId"] == "stale_patch"
            && event.payload["errorCode"] == "workspace.patch_stale_file"
    }));
    let commits: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "chat_commit_requested")
        .collect();
    assert_eq!(
        commits
            .iter()
            .map(|event| (
                event.payload["callId"].as_str().unwrap(),
                event.payload["path"].as_str().unwrap(),
                event.payload["isExplicit"].as_bool().unwrap(),
            ))
            .collect::<Vec<_>>(),
        [
            ("prepare", "output/Moved.MD", false),
            ("move_final", "scratch/finalized/final.md", false),
            ("commit", "scratch/finalized/final.md", true),
        ],
    );
    assert_eq!(commits[0].payload["sha256"], sha256_hex(b"shell draft\n"));
    assert_eq!(
        commits[1].payload["sha256"],
        sha256_hex(final_text.as_bytes())
    );
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&run.id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Completed,
    );
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn failed_shell_keeps_its_writes_without_publishing_an_earlier_candidate() {
    let root = temp_root("agent-shell-failure");
    let fixture = agent_runtime_fixture_with_responses(
        &root,
        vec![
            model_tool_response(vec![
                model_tool_call(
                    "initial_text",
                    "workspace_write_file",
                    json!({"path": "output/main.md", "content": "old candidate"}),
                ),
                model_tool_call(
                    "failed_shell",
                    "workspace_shell",
                    json!({"command": r#"python3 -c 'from pathlib import Path; Path("output/main.md").write_text("saved despite failure"); raise ValueError("draft incomplete")'"#}),
                ),
            ]),
            model_tool_response(vec![
                model_tool_call("commit", "workspace_commit", json!({})),
                model_tool_call("finish", "workspace_finish", json!({})),
            ]),
        ],
    );
    let mut profile = resolve_contract_profile(&fixture).await;
    profile.run.stream = false;
    let run = contract_run("shell_failure", AgentRunPresentation::Foreground, &profile);
    fixture.agent_repository.create_run(&run).await.unwrap();
    let request = chat_request("recover a draft after a failed shell command");
    let prompt_snapshot = json!({"chatCompletionPayload": request.payload.clone()});
    let (_cancel_sender, mut cancel_receiver) = watch::channel(false);
    execute_agent_loop_with_host_resolver(
        fixture.service.clone(),
        run.id.clone(),
        prompt_snapshot,
        request,
        profile,
        &mut cancel_receiver,
        resolve_chat_commits_and_persistent_state_update(
            fixture.service.clone(),
            fixture.agent_repository.clone(),
            run.id.clone(),
            "message_shell",
            &[],
        ),
    )
    .await
    .unwrap();

    let events = read_agent_events(&fixture.agent_repository, &run.id).await;
    let commits: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "chat_commit_requested")
        .collect();
    assert_eq!(
        commits.len(),
        1,
        "a failed shell must not auto-publish the old or updated candidate"
    );
    assert_eq!(commits[0].payload["callId"], "commit");
    assert_eq!(commits[0].payload["isExplicit"], true);
    assert_eq!(
        commits[0].payload["sha256"],
        sha256_hex(b"saved despite failure")
    );
    let result_event = events
        .iter()
        .find(|event| {
            event.event_type == "tool_result_stored" && event.payload["callId"] == "failed_shell"
        })
        .unwrap();
    let result = read_workspace_json(
        &fixture.agent_repository,
        &run.id,
        result_event.payload["path"].as_str().unwrap(),
    )
    .await;
    assert_eq!(result["structured"]["exitCode"], 1);
    assert_eq!(result["isError"], true);
    assert_eq!(
        fixture
            .agent_repository
            .load_run(&run.id)
            .await
            .unwrap()
            .status,
        AgentRunStatus::Completed,
    );
    fs::remove_dir_all(root).await.unwrap();
}

struct CancelledShell {
    started: tokio::sync::Notify,
    calls: AtomicUsize,
}

#[async_trait]
impl WorkspaceShell for CancelledShell {
    async fn execute(
        &self,
        mut request: WorkspaceShellRequest,
    ) -> Result<WorkspaceShellResult, DomainError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        request
            .files
            .write_file(
                &WorkspacePath::parse("output/main.md")?,
                b"cancelled shell write",
                WorkspaceWriteGuard::Unchecked,
            )
            .await?;
        self.started.notify_one();
        request
            .cancel
            .wait_for(|cancelled| *cancelled)
            .await
            .unwrap();
        Ok(WorkspaceShellResult {
            stdout: String::new(),
            stderr: String::new(),
            exit: WorkspaceShellExit::Cancelled,
            output_truncated: false,
        })
    }
}

#[tokio::test]
async fn cancelled_shell_records_its_result_and_resumes_after_the_confirmed_call() {
    let root = temp_root("agent-shell-cancel");
    let shell = Arc::new(CancelledShell {
        started: tokio::sync::Notify::new(),
        calls: AtomicUsize::new(0),
    });
    let fixture = agent_runtime_fixture_with_shell(
        &root,
        vec![Ok(model_tool_response(vec![
            model_tool_call(
                "cancel_shell",
                "workspace_shell",
                json!({"command": "write then wait"}),
            ),
            model_tool_call(
                "after_shell",
                "workspace_write_file",
                json!({
                    "path": "output/main.md", "content": " + resumed", "mode": "append",
                }),
            ),
            model_tool_call("finish", "workspace_finish", json!({})),
        ]))],
        shell.clone(),
    );
    let profile = resolve_contract_profile(&fixture).await;
    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "shell-cancel",
        Some(false),
    )
    .await;
    tokio::time::timeout(AGENT_CONTRACT_ASYNC_TIMEOUT, shell.started.notified())
        .await
        .unwrap();
    fixture
        .service
        .cancel_run(AgentCancelRunDto {
            run_id: handle.run_id.clone(),
        })
        .await
        .unwrap();
    let checkpoint = super::resume::wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(checkpoint.run.status, AgentRunStatus::Cancelled);
    assert!(checkpoint.blocked_reason.is_none());
    let events = read_agent_events(&fixture.agent_repository, &handle.run_id).await;
    let stored = events
        .iter()
        .find(|event| {
            event.event_type == "tool_result_stored" && event.payload["callId"] == "cancel_shell"
        })
        .expect("cancelled shell has a confirmed tool result");
    let result = read_workspace_json(
        &fixture.agent_repository,
        &handle.run_id,
        stored.payload["path"].as_str().unwrap(),
    )
    .await;
    assert_eq!(result["errorCode"], "workspace.shell_cancelled");
    assert!(
        !events
            .iter()
            .any(|event| event.payload["callId"] == "after_shell")
    );
    let run = fixture
        .agent_repository
        .load_run(&handle.run_id)
        .await
        .unwrap();
    fixture
        .service
        .resume_run(AgentResumeRunDto {
            run_id: handle.run_id.clone(),
            expected_terminal_seq: checkpoint.terminal_seq,
            chat_ref: run.chat_ref,
            stable_chat_id: run.stable_chat_id,
            additional_rounds: 0,
            host_presentation: false,
            revision: None,
        })
        .await
        .unwrap();
    let completed = super::resume::wait_for_checkpoint(&fixture, &handle.run_id).await;
    assert_eq!(completed.run.status, AgentRunStatus::Completed);
    assert_eq!(
        shell.calls.load(Ordering::SeqCst),
        1,
        "resume must not replay the cancelled shell"
    );
    assert_eq!(fixture.model_gateway.requests().await.len(), 1);
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
        "cancelled shell write + resumed",
    );
    fs::remove_dir_all(root).await.unwrap();
}
