use super::*;
use tt_domain::models::chat::ChatMessage;

fn assert_format_error(error: DomainError) {
    assert!(matches!(error, DomainError::InvalidData(message) if message != "integrity"));
}

#[tokio::test]
async fn commit_rejects_invalid_headers_even_when_forced_and_preserves_the_target() {
    let (repository, root) = setup_repository().await;
    let target = character_target("alice", "format");
    let valid = "\n\u{feff}\n{\"chat_metadata\":{\"integrity\":\" \\t\\n\"}}";
    commit_payload_bytes(&repository, target.clone(), valid.as_bytes(), false)
        .await
        .unwrap();
    let path = repository
        .get_chat_payload_path("alice", "format")
        .await
        .unwrap();

    for incoming in [
        b"".as_slice(),
        b"[]",
        b"{broken}\n{}",
        "\u{feff}\u{feff}{}".as_bytes(),
        "\u{feff}\n\u{feff}{}".as_bytes(),
        b"{\"unknown\":\"\xff\"}",
        b"{\"chat_metadata\":{\"integrity\":\"\"}}",
        b"{\"chat_metadata\":{\"integrity\":null}}",
    ] {
        assert_format_error(
            commit_payload_bytes(&repository, target.clone(), incoming, true)
                .await
                .unwrap_err(),
        );
        assert_eq!(fs::read(&path).await.unwrap(), valid.as_bytes());
    }

    fs::write(&path, b"[]").await.unwrap();
    assert_format_error(
        commit_payload_bytes(&repository, target.clone(), valid.as_bytes(), false)
            .await
            .unwrap_err(),
    );
    assert_eq!(fs::read(&path).await.unwrap(), b"[]");
    commit_payload_bytes(&repository, target, valid.as_bytes(), true)
        .await
        .unwrap();
    assert_eq!(fs::read(&path).await.unwrap(), valid.as_bytes());
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn empty_chat_reads_and_header_only_writes_keep_fields_open() {
    let (repository, root) = setup_repository().await;
    let target = character_target("alice", "format");
    commit_payload_bytes(&repository, target.clone(), b"{}", false)
        .await
        .unwrap();
    let path = repository
        .get_chat_payload_path("alice", "format")
        .await
        .unwrap();
    for empty in ["", " \r\n\u{feff}\n\t"] {
        fs::write(&path, empty).await.unwrap();
        assert!(
            repository
                .get_chat("alice", "format")
                .await
                .unwrap()
                .messages
                .is_empty()
        );
        commit_payload_bytes(&repository, target.clone(), b"{}", false)
            .await
            .unwrap();
        assert_eq!(
            repository
                .get_chat_payload("alice", "format")
                .await
                .unwrap(),
            vec![json!({})]
        );
    }
    let header = json!({"chat_metadata": 42, "user_name": [], "future": {"anything": true}});
    commit_payload_bytes(&repository, target, header.to_string().as_bytes(), false)
        .await
        .unwrap();
    assert_eq!(
        repository
            .get_chat_payload("alice", "format")
            .await
            .unwrap(),
        vec![header]
    );
    cleanup_repository(repository, root).await;
}

#[tokio::test]
async fn full_commit_leaves_body_validation_to_readers_and_failed_reads_cannot_rewrite_history() {
    let (repository, root) = setup_repository().await;
    let target = character_target("alice", "format");
    for body in [b"not-json".as_slice(), b"[]", b"{\"mes\":\"\xff\"}"] {
        let payload = [
            b"{}\n{\"mes\":\"before\"}\n".as_slice(),
            body,
            b"\n{\"mes\":\"after\"}",
        ]
        .concat();
        commit_payload_bytes(&repository, target.clone(), &payload, false)
            .await
            .unwrap();
        assert_format_error(
            repository
                .get_chat_payload("alice", "format")
                .await
                .unwrap_err(),
        );
        assert_format_error(
            repository
                .add_message(
                    "alice",
                    "format",
                    ChatMessage::character("Alice", "new message"),
                )
                .await
                .unwrap_err(),
        );
        assert_eq!(
            repository
                .get_chat_payload_bytes("alice", "format")
                .await
                .unwrap(),
            payload
        );
    }
    cleanup_repository(repository, root).await;
}
