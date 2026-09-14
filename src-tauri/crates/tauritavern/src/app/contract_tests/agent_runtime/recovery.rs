use super::*;

#[tokio::test]
async fn startup_recovery_marks_interrupted_nonterminal_runs_as_cancelled() {
    let root = temp_root("agent-startup-recovery");
    let fixture = agent_runtime_fixture(&root);
    let profile = resolve_contract_profile(&fixture).await;

    // Simulate a run left nonterminal by a killed process (no active handle exists).
    let interrupted = contract_run("run_interrupted", AgentRunPresentation::Background, &profile);
    fixture
        .agent_repository
        .create_run(&interrupted)
        .await
        .unwrap();
    let terminal = {
        let mut run = contract_run("run_terminal", AgentRunPresentation::Background, &profile);
        run.status = AgentRunStatus::Completed;
        run
    };
    fixture.agent_repository.create_run(&terminal).await.unwrap();

    fixture
        .service
        .recover_interrupted_runs()
        .await
        .expect("recover interrupted runs");

    let recovered = fixture
        .agent_repository
        .load_run("run_interrupted")
        .await
        .unwrap();
    assert_eq!(recovered.status, AgentRunStatus::Cancelled);
    let unchanged = fixture
        .agent_repository
        .load_run("run_terminal")
        .await
        .unwrap();
    assert_eq!(unchanged.status, AgentRunStatus::Completed);

    // The recovery decision must be visible in the run journal.
    let events = fixture
        .agent_repository
        .read_all_events("run_interrupted")
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "run_interrupted_recovered"),
        "recovery event must be journaled, got {events:?}"
    );

    // A second pass must be a no-op.
    fixture
        .service
        .recover_interrupted_runs()
        .await
        .expect("second recovery pass");
    let still_cancelled = fixture
        .agent_repository
        .load_run("run_interrupted")
        .await
        .unwrap();
    assert_eq!(still_cancelled.status, AgentRunStatus::Cancelled);

    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn startup_recovery_keeps_active_runs_untouched() {
    let root = temp_root("agent-startup-recovery-active");
    let fixture = agent_runtime_fixture(&root);
    let profile = resolve_contract_profile(&fixture).await;

    let handle = start_contract_agent_run(
        &fixture,
        &profile,
        AgentRunPresentation::Background,
        "recovery-active",
        Some(false),
    )
    .await;
    let started = fixture
        .agent_repository
        .load_run(&handle.run_id)
        .await
        .unwrap();
    assert!(!started.status.is_terminal());

    fixture
        .service
        .recover_interrupted_runs()
        .await
        .expect("recover interrupted runs");

    let still_active = fixture
        .agent_repository
        .load_run(&handle.run_id)
        .await
        .unwrap();
    assert_eq!(still_active.status, started.status);

    fs::remove_dir_all(root).await.unwrap();
}
