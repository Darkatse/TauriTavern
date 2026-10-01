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
    let header = json!({
        "chat_metadata": 42, "user_name": [], "future": {"anything": true},
        "mes": "header is not a message", "send_date": "2099-01-01T00:00:00Z",
    });
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
    let recent = repository
        .list_recent_chat_summaries(Some("alice"), false, 10, &[])
        .await
        .unwrap();
    let modified =
        FileChatRepository::file_signature_from_metadata(&fs::metadata(&path).await.unwrap())
            .modified_millis;
    assert_eq!(recent[0].message_count, 0);
    assert!(recent[0].preview.is_empty());
    assert_eq!(recent[0].date, modified);

    // Missing metadata and explicit null are distinct response shapes. The
    // non-object metadata case above already guards the open container contract.
    for metadata in [None, Some(Value::Null)] {
        let mut header = json!({});
        if let Some(value) = metadata.as_ref() {
            header["chat_metadata"] = value.clone();
        }
        commit_payload_bytes(
            &repository,
            character_target("alice", "format"),
            header.to_string().as_bytes(),
            true,
        )
        .await
        .unwrap();
        let full = repository
            .get_character_chat_summary("alice", "format", true)
            .await
            .unwrap();
        assert_eq!(full.chat_metadata, metadata);
    }
    // All readers select the final metadata object before validating identity.
    // The overwritten null must not make a valid final header unreadable.
    let duplicate_header = br#"{"chat_metadata":{"integrity":null},"chat_metadata":{"integrity":" final identity ","chat_id_hash":17}}"#;
    commit_payload_bytes(
        &repository,
        character_target("alice", "format"),
        duplicate_header,
        true,
    )
    .await
    .unwrap();
    let full = repository
        .get_chat_payload("alice", "format")
        .await
        .unwrap();
    let summary = repository
        .get_character_chat_summary("alice", "format", true)
        .await
        .unwrap();
    assert_eq!(summary.chat_metadata.as_ref(), full[0].get("chat_metadata"));
    assert_eq!(
        repository
            .get_character_chat_integrity("alice", "format")
            .await
            .unwrap()
            .as_deref(),
        full[0]["chat_metadata"]["integrity"].as_str()
    );
    assert_eq!(
        repository
            .search_chats("format", Some("alice"))
            .await
            .unwrap()[0]
            .chat_id,
        summary.chat_id
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
