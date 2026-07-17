use super::*;

use tt_application::dto::chat_history_dto::{ChatHistoryLocator, CurrentCommitReason};
use tt_application::services::chat_payload_commit_service::ChatPayloadCommitService;
use tt_ports::repositories::chat_payload_commit_repository::ChatPayloadCommitRepository;

#[tokio::test]
async fn chat_payload_commit_notifies_history_only_after_successful_publish() {
    let root = temp_root("chat-payload-commit-history");
    let default_user = root.join("default-user");
    let repository = Arc::new(FileChatRepository::with_chat_aliases(
        default_user.join("characters"),
        default_user.join("chats"),
        default_user.join("group chats"),
        default_user.join("backups"),
        new_shared_chat_alias_store_for_user_dir(&default_user),
    ));
    let coordinator = Arc::new(ChatHistoryCoordinator::new(
        repository.clone() as Arc<dyn ChatRepository>,
        repository.clone() as Arc<dyn GroupChatRepository>,
    ));
    let service = ChatPayloadCommitService::new(
        repository as Arc<dyn ChatPayloadCommitRepository>,
        coordinator.clone(),
    );
    let target = ChatHistoryLocator::Character {
        character_id: "Alice".to_string(),
        file_name: "Story".to_string(),
    };
    let payload = br#"{"user_name":"User","character_name":"Alice","chat_metadata":{}}"#;

    let successful = service
        .begin(target.clone(), false)
        .await
        .expect("begin successful commit");
    service
        .append(&successful.session_id, 0, payload)
        .await
        .expect("append successful commit");
    service
        .finish(
            &successful.session_id,
            payload.len() as u64,
            CurrentCommitReason::Mutation,
        )
        .await
        .expect("finish successful commit");
    assert_eq!(coordinator.commit_sequence_for_test().await, 1);
    service
        .finish(
            &successful.session_id,
            payload.len() as u64,
            CurrentCommitReason::Mutation,
        )
        .await
        .expect_err("double finish must reject consumed session");
    assert_eq!(coordinator.commit_sequence_for_test().await, 1);

    coordinator.invalidate_all_pending().await;
    let rejected = service
        .begin(target, false)
        .await
        .expect("begin rejected commit");
    service
        .append(&rejected.session_id, 0, payload)
        .await
        .expect("append rejected commit");
    service
        .finish(
            &rejected.session_id,
            payload.len() as u64 + 1,
            CurrentCommitReason::Mutation,
        )
        .await
        .expect_err("size mismatch must reject commit");
    assert_eq!(coordinator.commit_sequence_for_test().await, 1);

    fs::remove_dir_all(root).await.expect("remove test root");
}
