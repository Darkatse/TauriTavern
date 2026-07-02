use super::*;

#[tokio::test]
async fn character_service_exports_real_png_card_metadata() {
    let root = temp_root("character-png");
    let service = character_service(&root).await;
    let card = character_card("Alice", json!({ "custom": "kept" }));

    service
        .create_character(create_character("Alice", Some(card)))
        .await
        .expect("create character");

    let stored_png = fs::read(root.join("default-user/characters/Alice.png"))
        .await
        .expect("read stored character png");
    assert!(stored_png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let stored_card = read_card_json(&stored_png);
    assert_eq!(stored_card.pointer("/unknownTop/kept"), Some(&json!(true)));
    assert_eq!(
        stored_card.pointer("/data/extensions/custom"),
        Some(&json!("kept"))
    );

    let exported = service
        .export_character_content(ExportCharacterContentDto {
            name: "Alice".to_string(),
            format: "png".to_string(),
        })
        .await
        .expect("export png content");
    let exported_card = read_card_json(&exported.data);
    assert_eq!(exported.mime_type, "image/png");
    assert_eq!(
        exported_card.pointer("/data/extensions/custom"),
        Some(&json!("kept"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_returns_raw_json_and_v2_data_metadata_from_real_png() {
    let root = temp_root("character-raw-json");
    let service = character_service(&root).await;
    let mut card = character_card("Alice", json!({ "custom": "kept" }));
    card["creator"] = json!("stale root creator");
    card["creator_notes"] = json!("stale root notes");
    card["character_version"] = json!("stale root version");
    card["data"]["creator"] = json!("data creator");
    card["data"]["creator_notes"] = json!("data notes");
    card["data"]["character_version"] = json!("data version");

    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write character png");

    let dto = service.get_character("Alice").await.expect("get character");
    let raw_json: Value =
        serde_json::from_str(&dto.json_data.expect("raw json data")).expect("parse raw json");
    assert_eq!(raw_json.pointer("/unknownTop/kept"), Some(&json!(true)));
    assert_eq!(dto.creator, "data creator");
    assert_eq!(dto.creator_notes, "data notes");
    assert_eq!(dto.character_version, "data version");

    let listed = service
        .get_all_characters(true)
        .await
        .expect("list characters");
    assert_eq!(listed[0].creator, "data creator");

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_embeds_create_time_primary_lorebook_into_real_card_png() {
    let root = temp_root("character-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info(
            "Lore",
            &json!({
                "entries": {
                    "1": {
                        "uid": 1,
                        "key": ["alpha"],
                        "comment": "memo",
                        "content": "lore text",
                        "order": 0,
                        "position": 0,
                        "disable": false
                    }
                }
            }),
        )
        .await
        .expect("save world info");

    service
        .create_character(CreateCharacterDto {
            primary_lorebook: Some("Lore".to_string()),
            ..create_character("Alice", None)
        })
        .await
        .expect("create character with primary lorebook");

    let stored_png = fs::read(root.join("default-user/characters/Alice.png"))
        .await
        .expect("read stored character png");
    let stored_card = read_card_json(&stored_png);

    assert_eq!(
        stored_card.pointer("/data/character_book/name"),
        Some(&json!("Lore"))
    );
    assert_eq!(
        stored_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("lore text"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_update_preserves_v3_spec_and_unknown_fields() {
    let root = temp_root("character-update-v3");
    let service = character_service(&root).await;
    let mut card = character_card("Alice", json!({ "custom": "kept" }));
    card["spec"] = json!("chara_card_v3");
    card["spec_version"] = json!("3.0");

    service
        .create_character(create_character("Alice", Some(card)))
        .await
        .expect("create v3 character");

    service
        .update_character(
            "Alice",
            UpdateCharacterDto {
                description: Some("updated description".to_string()),
                extensions: Some(json!({ "extra": "new" })),
                ..empty_update_character()
            },
        )
        .await
        .expect("update character");

    let stored_card = read_stored_card(&root, "Alice").await;
    assert_eq!(stored_card.get("spec"), Some(&json!("chara_card_v3")));
    assert_eq!(stored_card.get("spec_version"), Some(&json!("3.0")));
    assert_eq!(
        stored_card.pointer("/description"),
        Some(&json!("updated description"))
    );
    assert_eq!(
        stored_card.pointer("/data/description"),
        Some(&json!("updated description"))
    );
    assert_eq!(stored_card.pointer("/unknownTop/kept"), Some(&json!(true)));
    assert_eq!(
        stored_card.pointer("/data/extensions/custom"),
        Some(&json!("kept"))
    );
    assert_eq!(
        stored_card.pointer("/data/extensions/extra"),
        Some(&json!("new"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_raw_card_update_materializes_current_bound_lorebook() {
    let root = temp_root("character-update-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("Lore", &world_info("current lore"))
        .await
        .expect("save world info");
    service
        .create_character(create_character("Alice", None))
        .await
        .expect("create character");

    let mut card = character_card("Alice", json!({ "world": "Lore" }));
    card["data"]["character_book"] = json!({
        "name": "Lore",
        "entries": [{
            "uid": 1,
            "key": ["old"],
            "content": "stale lore",
            "extensions": {}
        }]
    });
    service
        .update_character_card_data(
            "Alice",
            UpdateCharacterCardDataDto {
                card_json: serde_json::to_string(&card).expect("serialize card"),
                avatar_path: None,
                crop: None,
            },
        )
        .await
        .expect("update raw card data");

    let stored_card = read_stored_card(&root, "Alice").await;
    assert_eq!(
        stored_card.pointer("/data/character_book/name"),
        Some(&json!("Lore"))
    );
    assert_eq!(
        stored_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("current lore"))
    );
    assert_eq!(
        stored_card.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_export_sanitizes_private_fields_and_materializes_current_lorebook() {
    let root = temp_root("character-export-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("Lore", &world_info("current lore"))
        .await
        .expect("save current world info");
    let mut card = character_card(
        "Alice",
        json!({
            "world": "Lore",
            "fav": true,
            "tauritavern": {
                "agentProfiles": {
                    "version": 1,
                    "items": [{
                        "profile": {
                            "id": "embedded-writer",
                            "model": {
                                "mode": "connectionRef",
                                "connectionRef": "secret-connection",
                                "modelId": "secret-model"
                            }
                        }
                    }]
                }
            }
        }),
    );
    card["chat"] = json!("private-chat");
    card["fav"] = json!(true);
    card["data"]["character_book"] = character_book("Lore", "stale embedded lore");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write stale character card");

    let exported = service
        .export_character_content(ExportCharacterContentDto {
            name: "Alice".to_string(),
            format: "png".to_string(),
        })
        .await
        .expect("export png");
    let exported_card = read_card_json(&exported.data);
    assert_eq!(exported_card.get("fav"), Some(&json!(false)));
    assert!(exported_card.get("chat").is_none());
    assert_eq!(
        exported_card.pointer("/data/extensions/fav"),
        Some(&json!(false))
    );
    assert_eq!(
        exported_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("current lore"))
    );
    assert_eq!(
        exported_card.pointer("/data/extensions/tauritavern/agentProfiles/items/0/profile/model"),
        Some(&json!({ "mode": "requiresConfiguration" }))
    );

    let export_path = root.join("exported.json");
    service
        .export_character(ExportCharacterDto {
            name: "Alice".to_string(),
            target_path: export_path.to_string_lossy().to_string(),
        })
        .await
        .expect("export file");
    let exported_file: Value =
        serde_json::from_slice(&fs::read(export_path).await.expect("read exported file"))
            .expect("parse exported file");
    assert_eq!(
        exported_file.pointer("/data/character_book/entries/0/content"),
        Some(&json!("current lore"))
    );

    let source_card = read_stored_card(&root, "Alice").await;
    assert_eq!(
        source_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("stale embedded lore"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_update_avatar_materializes_current_lorebook_into_written_card() {
    let root = temp_root("character-avatar-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("Lore", &world_info("current avatar lore"))
        .await
        .expect("save current world info");
    let mut card = character_card("Alice", json!({ "world": "Lore" }));
    card["data"]["character_book"] = character_book("Lore", "stale avatar lore");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write stale character card");
    let replacement_avatar = root.join("replacement.png");
    fs::write(&replacement_avatar, minimal_png())
        .await
        .expect("write replacement avatar");

    service
        .update_avatar(UpdateAvatarDto {
            name: "Alice".to_string(),
            avatar_path: replacement_avatar.to_string_lossy().to_string(),
            crop: None,
        })
        .await
        .expect("update avatar");

    let stored_card = read_stored_card(&root, "Alice").await;
    assert_eq!(
        stored_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("current avatar lore"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_lorebook_conflict_resolution_uses_selected_source() {
    let root = temp_root("character-lorebook-conflict");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("CurrentLore", &world_info("current conflict lore"))
        .await
        .expect("save current world info");
    let mut current_card = character_card("Alice", json!({ "world": "CurrentLore" }));
    current_card["data"]["character_book"] = character_book("CurrentLore", "stale conflict lore");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&current_card),
    )
    .await
    .expect("write current-resolution card");

    let conflict = service
        .check_lorebook_conflict(CheckCharacterLorebookConflictDto {
            name: "Alice".to_string(),
        })
        .await
        .expect("check current conflict");
    assert!(conflict.conflict);
    assert!(conflict.current_available);

    service
        .resolve_lorebook_conflict(ResolveCharacterLorebookConflictDto {
            name: "Alice".to_string(),
            resolution: CharacterLorebookConflictResolution::Current,
        })
        .await
        .expect("resolve with current world");
    let resolved_card = read_stored_card(&root, "Alice").await;
    assert_eq!(
        resolved_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("current conflict lore"))
    );

    world_repository
        .save_world_info("EmbeddedLore", &world_info("old world lore"))
        .await
        .expect("save stale world info");
    let mut embedded_card = character_card("Bob", json!({ "world": "EmbeddedLore" }));
    embedded_card["data"]["character_book"] =
        character_book("EmbeddedLore", "embedded conflict lore");
    fs::write(
        root.join("default-user/characters/Bob.png"),
        character_png(&embedded_card),
    )
    .await
    .expect("write embedded-resolution card");

    service
        .resolve_lorebook_conflict(ResolveCharacterLorebookConflictDto {
            name: "Bob".to_string(),
            resolution: CharacterLorebookConflictResolution::Embedded,
        })
        .await
        .expect("resolve with embedded book");
    let overwritten = world_repository
        .get_world_info("EmbeddedLore", false)
        .await
        .expect("read overwritten world")
        .expect("world exists");
    assert!(
        overwritten
            .get("entries")
            .and_then(Value::as_object)
            .expect("world entries")
            .values()
            .any(|entry| entry.get("content") == Some(&json!("embedded conflict lore")))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_import_auto_links_embedded_lorebook_without_dropping_unknown_fields() {
    let root = temp_root("character-import-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    let mut card = character_card("Alice", json!({ "custom": "kept" }));
    card["data"]["character_book"] = json!({
        "name": "Embedded Lore",
        "entries": [{
            "id": 1,
            "keys": ["alpha"],
            "content": "embedded lore",
            "enabled": true,
            "extensions": {}
        }],
        "extensions": {}
    });
    let import_path = root.join("import.png");
    fs::write(&import_path, character_png(&card))
        .await
        .expect("write import png");

    let imported = service
        .import_character(ImportCharacterDto {
            file_path: import_path.to_string_lossy().to_string(),
            preserve_file_name: None,
        })
        .await
        .expect("import character");

    let stem = imported.avatar.trim_end_matches(".png");
    let stored_card = read_stored_card(&root, stem).await;
    assert_eq!(
        stored_card.pointer("/data/extensions/world"),
        Some(&json!("Embedded Lore"))
    );
    assert_eq!(stored_card.pointer("/unknownTop/kept"), Some(&json!(true)));
    assert_eq!(
        stored_card.pointer("/data/extensions/custom"),
        Some(&json!("kept"))
    );
    let world = world_repository
        .get_world_info("Embedded Lore", false)
        .await
        .expect("read world info")
        .expect("world info imported");
    assert_eq!(
        world.pointer("/entries/1/content"),
        Some(&json!("embedded lore"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_single_merge_preserves_unknown_fields_and_rejects_invalid_cards() {
    let root = temp_root("character-single-merge");
    let service = character_service(&root).await;
    service
        .create_character(create_character(
            "Alice",
            Some(character_card("Alice", json!({ "custom": "kept" }))),
        ))
        .await
        .expect("create character");

    service
        .merge_character_card_data(
            "Alice",
            MergeCharacterCardDataDto {
                update: json!({
                    "description": "merged description",
                    "data": {
                        "description": "merged description",
                        "extensions": {
                            "newFlag": true
                        }
                    }
                }),
            },
        )
        .await
        .expect("merge card data");
    let merged = read_stored_card(&root, "Alice").await;
    assert_eq!(
        merged.pointer("/data/description"),
        Some(&json!("merged description"))
    );
    assert_eq!(merged.pointer("/unknownTop/kept"), Some(&json!(true)));
    assert_eq!(
        merged.pointer("/data/extensions/newFlag"),
        Some(&json!(true))
    );

    let error = service
        .merge_character_card_data(
            "Alice",
            MergeCharacterCardDataDto {
                update: json!({
                    "data": {
                        "extensions": "not an object"
                    }
                }),
            },
        )
        .await
        .expect_err("strict merge rejects invalid card");
    assert!(matches!(error, ApplicationError::ValidationError(_)));

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_bulk_merge_filters_and_unsets_with_real_card_writes() {
    let root = temp_root("character-bulk-merge");
    let service = character_service(&root).await;
    service
        .create_character(create_character(
            "Alice",
            Some(character_card(
                "Alice",
                json!({ "world": "Lore", "bulkTarget": true }),
            )),
        ))
        .await
        .expect("create alice");
    service
        .create_character(create_character(
            "Bob",
            Some(character_card("Bob", json!({}))),
        ))
        .await
        .expect("create bob");

    let result = service
        .bulk_merge_character_card_data(BulkMergeCharacterCardDataDto {
            avatars: Vec::new(),
            data: json!({
                "data": {
                    "extensions": {
                        "fav": true,
                        "world": "__@@UNSET@@__"
                    }
                }
            }),
            filter: Some(BulkMergeCharacterCardDataFilterDto {
                path: "data.extensions.bulkTarget".to_string(),
            }),
        })
        .await
        .expect("bulk merge");

    assert_eq!(result.updated, vec!["Alice.png"]);
    assert_eq!(result.skipped, vec!["Bob.png"]);
    assert!(result.failed.is_empty());
    let alice = read_stored_card(&root, "Alice").await;
    assert_eq!(alice.pointer("/data/extensions/fav"), Some(&json!(true)));
    assert!(alice.pointer("/data/extensions/world").is_none());
    let bob = read_stored_card(&root, "Bob").await;
    assert_ne!(bob.pointer("/data/extensions/fav"), Some(&json!(true)));

    let _ = fs::remove_dir_all(root).await;
}
