use std::sync::Arc;

use super::*;
use serde_json::json;
use tt_domain::models::agent::AgentModelMessage;
use tt_domain::models::agent::session::AgentSession;
use tt_domain::models::agent::session::AgentSessionMessage;
use tt_ports::repositories::agent_session_repository::{
    AgentSessionMessageReadQuery, AgentSessionRepository,
};
use tt_ports::workspace_fs::WorkspaceFs;

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).unwrap()
}

async fn create_session(repository: &FileAgentRepository, id: &str) {
    repository
        .create_session(&AgentSession {
            id: id.into(),
            created_at: Utc::now(),
            title: None,
            last_used_at: None,
        })
        .await
        .unwrap();
}

async fn session_run(
    repository: &FileAgentRepository,
    session_id: &str,
    run_id: &str,
) -> (AgentRun, Arc<dyn WorkspaceFs>) {
    let mut run = sample_run_with_id(run_id);
    run.workspace_id = session_id.into();
    run.target = AgentRunTarget::Session {
        session_id: session_id.into(),
    };
    repository.create_run(&run).await.unwrap();
    let mut manifest = sample_manifest(&run);
    manifest.artifacts.clear();
    manifest.roots = ["work", "tmp", "tool-results"]
        .into_iter()
        .map(|name| WorkspaceRootSpec {
            path: name.into(),
            lifecycle: WorkspaceRootLifecycle::Persistent,
            scope: WorkspaceRootScope::Session,
            mount: WorkspaceRootMount::Materialized,
            visible: true,
            writable: name != "tool-results",
            commit: WorkspaceRootCommit::Never,
        })
        .collect();
    repository
        .initialize_run(
            &run,
            &manifest,
            &json!({"runId": run_id}),
            &sample_resolved_profile(&manifest),
        )
        .await
        .unwrap();
    let files = repository.open_filesystem(run_id).await.unwrap();
    (run, files)
}

#[tokio::test]
async fn session_workspace_survives_runs_and_reopen_without_joining_chat_lifecycle() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    create_session(&repository, "session_a").await;
    create_session(&repository, "session_b").await;
    let (first_run, first) = session_run(&repository, "session_a", "opaque_first").await;
    first
        .write_text(
            &path("work/note.md"),
            "shared",
            WorkspaceWriteGuard::Unchecked,
        )
        .await
        .unwrap();
    first
        .write_text(
            &path("tmp/check.js"),
            "temporary",
            WorkspaceWriteGuard::Unchecked,
        )
        .await
        .unwrap();
    first
        .write_text(
            &path("tool-results/opaque_first/result.txt"),
            "old result",
            WorkspaceWriteGuard::Unchecked,
        )
        .await
        .unwrap();

    let (_, second) = session_run(&repository, "session_a", "opaque_second").await;
    let (_, other) = session_run(&repository, "session_b", "opaque_other").await;
    assert!(matches!(
        other.read_text(&path("work/note.md")).await,
        Err(DomainError::NotFound(_))
    ));
    assert_ne!(
        first
            .read_text(&path("input/prompt_snapshot.json"))
            .await
            .unwrap()
            .text,
        second
            .read_text(&path("input/prompt_snapshot.json"))
            .await
            .unwrap()
            .text
    );

    let create_path = path("work/once.txt");
    let (left, right) = tokio::join!(
        first.write_file(&create_path, b"first", WorkspaceWriteGuard::MustNotExist),
        second.write_file(&create_path, b"second", WorkspaceWriteGuard::MustNotExist),
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert!(matches!(
        left.err().or(right.err()),
        Some(DomainError::WorkspaceWriteConflict { .. })
    ));
    second
        .rename(&path("tmp/check.js"), &path("work/check.js"))
        .await
        .unwrap();
    let roots = second.read_dir(None, 100).await.unwrap();
    for name in ["work", "tmp", "tool-results"] {
        assert_eq!(
            roots
                .iter()
                .filter(|entry| entry.path.as_str() == name)
                .count(),
            1
        );
    }

    let chat = sample_run();
    repository.create_run(&chat).await.unwrap();
    assert_eq!(
        repository
            .list_all_runs()
            .await
            .unwrap()
            .iter()
            .map(|run| run.id.as_str())
            .collect::<Vec<_>>(),
        [chat.id.as_str()]
    );
    repository
        .delete_chat_workspace(&chat.workspace_id)
        .await
        .unwrap();
    repository.delete_run(&first_run).await.unwrap();
    let reopened = FileAgentRepository::new(root.clone());
    let files = reopened.open_filesystem("opaque_second").await.unwrap();
    assert_eq!(
        files.read_text(&path("work/check.js")).await.unwrap().text,
        "temporary"
    );
    assert_eq!(
        files
            .read_text(&path("tool-results/opaque_first/result.txt"))
            .await
            .unwrap()
            .text,
        "old result"
    );
    assert!(reopened.load_session("session_a").await.is_ok());
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn session_history_pages_and_appends_across_reopen() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    create_session(&repository, "session_a").await;
    session_run(&repository, "session_a", "opaque_run").await;
    let message = |text: &str| {
        serde_json::from_value::<AgentModelMessage>(json!({
            "role": "user", "parts": [{"type": "text", "text": text}]
        }))
        .unwrap()
    };
    let first_message = message("first");
    let second_message = message("second");
    let third_message = message("third");
    repository
        .append_session_message("session_a", "opaque_run", &first_message, None)
        .await
        .unwrap();
    let (left, right) = tokio::join!(
        repository.append_session_message("session_a", "opaque_run", &second_message, None),
        repository.append_session_message("session_a", "opaque_run", &third_message, None),
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(left.seq, right.seq);
    assert_eq!(repository.session_last_seq("session_a").await.unwrap(), 3);
    let reopened = FileAgentRepository::new(root.clone());
    let recent = reopened
        .read_session_messages(
            "session_a",
            AgentSessionMessageReadQuery {
                after_seq: None,
                before_seq: None,
                limit: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        recent.iter().map(|entry| entry.seq).collect::<Vec<_>>(),
        [2, 3]
    );
    assert!(recent.iter().any(|entry| entry.message == second_message));
    assert!(recent.iter().any(|entry| entry.message == third_message));
    let earlier = reopened
        .read_session_messages(
            "session_a",
            AgentSessionMessageReadQuery {
                after_seq: None,
                before_seq: Some(2),
                limit: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(earlier.len(), 1);
    assert_eq!(earlier[0].message, first_message);
    assert_eq!(
        reopened
            .append_session_message("session_a", "opaque_run", &first_message, None)
            .await
            .unwrap()
            .seq,
        4
    );

    let history = root.join("sessions/session_a/history.jsonl");
    let mut broken = fs::read(&history).await.unwrap();
    broken.extend_from_slice(b"{\"seq\":5");
    fs::write(&history, &broken).await.unwrap();
    let reopened = FileAgentRepository::new(root.clone());
    assert!(reopened.session_last_seq("session_a").await.is_err());
    assert!(
        reopened
            .append_session_message("session_a", "opaque_run", &first_message, None)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&history).await.unwrap(), broken);
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn session_metadata_and_deletion_preserve_other_sessions_and_shared_profile() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    create_session(&repository, "session_a").await;
    create_session(&repository, "session_b").await;
    let created_at = Utc::now() - chrono::Duration::days(2);
    // Existing metadata has neither title nor lastUsedAt.
    for (id, at) in [
        ("session_a", created_at),
        ("session_b", created_at + chrono::Duration::days(1)),
    ] {
        fs::write(
            root.join("sessions").join(id).join("session.json"),
            serde_json::to_vec(&json!({"id": id, "createdAt": at})).unwrap(),
        )
        .await
        .unwrap();
    }
    let sessions = repository.list_sessions().await.unwrap();
    assert_eq!(
        sessions
            .iter()
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>(),
        ["session_b", "session_a"]
    );
    assert!(
        sessions
            .iter()
            .all(|session| session.title.is_none() && session.last_used_at.is_none())
    );

    session_run(&repository, "session_a", "run_a").await;
    let (other_run, _) = session_run(&repository, "session_b", "run_b").await;
    let user: AgentModelMessage = serde_json::from_value(json!({
        "role": "user", "parts": [{"type": "text", "text": "  检查\n  扩展的问题  "}]
    }))
    .unwrap();
    let first = repository
        .append_session_message("session_a", "run_a", &user, None)
        .await
        .unwrap();
    let sessions = repository.list_sessions().await.unwrap();
    assert_eq!(sessions[0].id, "session_a");
    assert_eq!(sessions[0].title.as_deref(), Some("检查 扩展的问题"));
    assert_eq!(sessions[0].last_used_at, Some(first.created_at));
    let renamed = repository
        .rename_session("session_a", "Saved title")
        .await
        .unwrap();
    assert_eq!(renamed.last_used_at, sessions[0].last_used_at);
    repository
        .append_session_message("session_a", "run_a", &user, None)
        .await
        .unwrap();
    let used_at = repository
        .load_session("session_a")
        .await
        .unwrap()
        .last_used_at;
    let assistant: AgentModelMessage = serde_json::from_value(json!({
        "role": "assistant", "parts": [{"type": "text", "text": "Done"}]
    }))
    .unwrap();
    repository
        .append_session_message("session_a", "run_a", &assistant, None)
        .await
        .unwrap();
    let reopened = FileAgentRepository::new(root.clone());
    let saved = reopened.load_session("session_a").await.unwrap();
    assert_eq!(saved.title.as_deref(), Some("Saved title"));
    assert_eq!(saved.last_used_at, used_at);

    // Listing and deletion need metadata, never transcripts or model/tool materials.
    fs::write(
        root.join("sessions/session_a/history.jsonl"),
        b"invalid history",
    )
    .await
    .unwrap();
    let shared_profile = root.join("sessions/profile.json");
    fs::write(&shared_profile, b"shared profile").await.unwrap();
    let summary = reopened.index_run_summary_path("run_a").unwrap();
    fs::create_dir_all(summary.parent().unwrap()).await.unwrap();
    fs::write(&summary, b"summary").await.unwrap();
    assert_eq!(reopened.list_sessions().await.unwrap().len(), 2);
    reopened.delete_session("session_a").await.unwrap();
    reopened.delete_session("session_a").await.unwrap();
    assert!(!root.join("sessions/session_a").exists());
    assert!(!reopened.index_session_run_path("run_a").unwrap().exists());
    assert!(!summary.exists());
    assert!(
        reopened
            .rename_session("session_a", "Cannot recreate")
            .await
            .is_err()
    );
    assert!(!root.join("sessions/session_a").exists());
    assert_eq!(reopened.load_run("run_b").await.unwrap().id, other_run.id);
    assert_eq!(reopened.list_sessions().await.unwrap()[0].id, "session_b");
    assert_eq!(fs::read(&shared_profile).await.unwrap(), b"shared profile");
    fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn full_session_history_for_prompt_assembly_is_not_capped_at_ui_page_size() {
    let root = temp_root();
    let repository = FileAgentRepository::new(root.clone());
    create_session(&repository, "session_a").await;
    let message: AgentModelMessage = serde_json::from_value(json!({
        "role": "user", "parts": [{"type": "text", "text": "message"}]
    }))
    .unwrap();
    let mut history = String::new();
    for seq in 1..=501 {
        history.push_str(
            &serde_json::to_string(&AgentSessionMessage {
                seq,
                run_id: "opaque_run".into(),
                created_at: Utc::now(),
                message: message.clone(),
                origin: None,
            })
            .unwrap(),
        );
        history.push('\n');
    }
    fs::write(root.join("sessions/session_a/history.jsonl"), history)
        .await
        .unwrap();
    let messages = repository
        .read_session_messages(
            "session_a",
            AgentSessionMessageReadQuery {
                after_seq: Some(0),
                before_seq: None,
                limit: usize::MAX,
            },
        )
        .await
        .unwrap();
    assert_eq!(messages.len(), 501);
    assert_eq!(
        messages.last().unwrap().seq,
        repository.session_last_seq("session_a").await.unwrap()
    );
    fs::remove_dir_all(root).await.unwrap();
}
