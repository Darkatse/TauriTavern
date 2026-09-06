use super::*;
use tt_application::dto::agent_dto::AgentReadTaskDetailDto;
use tt_domain::models::agent::AgentTaskRecord;

#[tokio::test]
async fn task_details_preserve_output_and_validate_persisted_sources_only_when_requested() {
    let root = temp_root("agent-task-details");
    let fixture = agent_runtime_fixture(&root);
    let profile = resolve_contract_profile(&fixture).await;
    let run = contract_run("task-details", AgentRunPresentation::Background, &profile);
    fixture.agent_repository.create_run(&run).await.unwrap();
    let result_path = WorkspacePath::parse("agent-results/child.json").unwrap();
    let mut task = AgentTaskRecord {
        id: "task-1".to_string(),
        run_id: run.id.clone(),
        parent_invocation_id: ROOT_AGENT_INVOCATION_ID.to_string(),
        child_invocation_id: "child".to_string(),
        target_profile_id: "critic".to_string(),
        workspace_key: "critic".to_string(),
        continuation: AgentDelegationContinuation::ReturnToParent,
        status: AgentTaskStatus::Completed,
        task: json!({"objective": "Review the scene.", "customConstraint": ["Keep the ending."]}),
        created_by_tool_call_id: None,
        result_ref: Some(result_path.as_str().to_string()),
        error: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    fixture.agent_repository.save_task(&task).await.unwrap();
    let query = AgentReadTaskDetailDto {
        run_id: run.id.clone(),
        task_id: task.id.clone(),
        include_result: true,
    };
    let mut result = json!({
        "schemaVersion": 1, "kind": "tauritavern.agentTaskResult",
        "summary": "Add a reason for her return.",
        "result": {"questionsForParent": ["May I mention the letter?"], "customFinding": {"scene": 3}},
        "runtime": {"runId": run.id, "taskId": task.id, "childInvocationId": task.child_invocation_id},
    });
    fixture
        .agent_repository
        .write_text(&run.id, &result_path, &result.to_string())
        .await
        .unwrap();
    let detail = fixture
        .service
        .read_task_detail(query.clone())
        .await
        .unwrap();
    assert_eq!(
        detail.task.details["customConstraint"],
        json!(["Keep the ending."])
    );
    let output = detail.result.expect("result requested").output;
    assert_eq!(
        output["questionsForCaller"],
        json!(["May I mention the letter?"])
    );
    assert!(!output.contains_key("questionsForParent"));
    assert_eq!(output["customFinding"], json!({"scene": 3}));

    result["runtime"]["taskId"] = json!("another-task");
    fixture
        .agent_repository
        .write_text(&run.id, &result_path, &result.to_string())
        .await
        .unwrap();
    let brief_query = AgentReadTaskDetailDto {
        include_result: false,
        ..query.clone()
    };
    let brief = fixture
        .service
        .read_task_detail(brief_query.clone())
        .await
        .unwrap();
    assert!(
        brief.result.is_none(),
        "brief reads must not load the mismatched result"
    );
    let error = fixture.service.read_task_detail(query).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("result identity does not match the task")
    );

    task.task = Value::Null;
    fixture.agent_repository.save_task(&task).await.unwrap();
    let error = fixture
        .service
        .read_task_detail(brief_query)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("agent.task_brief_invalid"));
    let _ = fs::remove_dir_all(root).await;
}
