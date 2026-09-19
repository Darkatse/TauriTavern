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
async fn character_service_create_keeps_character_when_lorebook_materialization_fails() {
    let root = temp_root("character-create-broken-lorebook");
    let service = character_service(&root).await;
    let worlds_dir = root.join("default-user/worlds");
    fs::create_dir_all(&worlds_dir)
        .await
        .expect("create worlds dir");
    fs::write(worlds_dir.join("Broken Lore.json"), b"not json")
        .await
        .expect("write broken world info");
    let mut dto = create_character("Alice", None);
    dto.primary_lorebook = Some("Broken Lore".to_string());
    dto.extensions = Some(json!({ "world": "Broken Lore" }));

    service
        .create_character(dto)
        .await
        .expect("create character without optional embedded lorebook");

    let stored = read_stored_card(&root, "Alice").await;
    assert_eq!(
        stored.pointer("/data/extensions/world"),
        Some(&json!("Broken Lore"))
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
    card["data"]["creator_notes"] = json!(9);
    card["data"]["system_prompt"] = json!(["system"]);
    card["data"]["alternate_greetings"] = json!("");
    card["data"]["extensions"]["world"] = json!(42);
    card["data"]["extensions"]["depth_prompt"] = json!({
        "prompt": "",
        "depth": "",
        "role": "system"
    });
    card["data"]["character_book"] = json!("opaque");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write v3 character");

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
    for (path, expected) in [
        ("/data/creator_notes", json!(9)),
        ("/data/system_prompt", json!(["system"])),
        ("/data/alternate_greetings", json!("")),
        ("/data/extensions/world", json!(42)),
        ("/data/extensions/depth_prompt/depth", json!("")),
        ("/data/character_book", json!("opaque")),
    ] {
        assert_eq!(stored_card.pointer(path), Some(&expected), "{path}");
    }

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_raw_card_update_preserves_embedded_lorebook_without_resolving_binding() {
    let root = temp_root("character-update-lorebook");
    let service = character_service(&root).await;
    service
        .create_character(create_character("Alice", None))
        .await
        .expect("create character");

    let mut card = character_card("Alice", json!({ "world": "Missing Lore" }));
    card["data"]["character_book"] = json!({
        "name": "Embedded Lore",
        "entries": [{
            "uid": 1,
            "key": ["old"],
            "content": "embedded lore",
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
                materialize_primary_lorebook: false,
            },
        )
        .await
        .expect("update raw card data");

    let stored_card = read_stored_card(&root, "Alice").await;
    assert_eq!(
        stored_card.pointer("/data/character_book/name"),
        Some(&json!("Embedded Lore"))
    );
    assert_eq!(
        stored_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("embedded lore"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_form_update_refreshes_available_lorebook_without_blocking_raw_updates() {
    let root = temp_root("character-form-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("Local Lore", &world_info("current lore"))
        .await
        .expect("save world info");
    service
        .create_character(create_character("Alice", None))
        .await
        .expect("create character");

    let mut card = character_card("Alice", json!({ "world": "Local Lore" }));
    card["data"]["character_book"] = character_book("Local Lore", "stale lore");
    service
        .update_character_card_data(
            "Alice",
            UpdateCharacterCardDataDto {
                card_json: serde_json::to_string(&card).expect("serialize card"),
                avatar_path: None,
                crop: None,
                materialize_primary_lorebook: true,
            },
        )
        .await
        .expect("update form card data");

    assert_eq!(
        read_stored_card(&root, "Alice")
            .await
            .pointer("/data/character_book/entries/0/content"),
        Some(&json!("current lore"))
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
async fn character_service_export_keeps_stored_card_when_optional_enrichment_is_invalid() {
    let root = temp_root("character-export-optional-fallback");
    let service = character_service(&root).await;
    let mut card = character_card("Alice", json!({ "world": "Missing Lore" }));
    card["data"]["character_book"] = character_book("Stored Lore", "stored lore");
    card["data"]["extensions"]["tauritavern"] = json!({
        "agentProfiles": { "version": 2, "items": [] },
        "kept": true
    });
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write character card");

    let exported = service
        .export_character_content(ExportCharacterContentDto {
            name: "Alice".to_string(),
            format: "json".to_string(),
        })
        .await
        .expect("export without optional enrichment");
    let exported_card: Value = serde_json::from_slice(&exported.data).expect("parse exported card");

    assert_eq!(
        exported_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("stored lore"))
    );
    assert!(
        exported_card
            .pointer("/data/extensions/tauritavern/agentProfiles")
            .is_none()
    );
    assert_eq!(
        exported_card.pointer("/data/extensions/tauritavern/kept"),
        Some(&json!(true))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_update_avatar_preserves_embedded_lorebook() {
    let root = temp_root("character-avatar-lorebook");
    let service = character_service(&root).await;
    let mut card = character_card("Alice", json!({ "world": "" }));
    card["data"]["character_book"] = character_book("Embedded Lore", "embedded avatar lore");
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
        Some(&json!("embedded avatar lore"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_lorebook_check_does_not_block_on_unrecognized_embedded_data() {
    let root = temp_root("character-lorebook-check-open-data");
    let service = character_service(&root).await;
    let mut card = character_card("Alice", json!({ "world": "Lore" }));
    card["data"]["character_book"] = json!({
        "name": "Opaque Lore",
        "entries": "not a supported entry set"
    });
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write character card");

    let conflict = service
        .check_lorebook_conflict(CheckCharacterLorebookConflictDto {
            name: "Alice".to_string(),
        })
        .await
        .expect("continue when optional lorebook data cannot be compared");

    assert!(!conflict.conflict);
    assert_eq!(conflict.world, "Lore");
    assert_eq!(conflict.embedded_name.as_deref(), Some("Opaque Lore"));

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_lorebook_conflict_resolution_uses_current_or_embedded_source() {
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

    let current_result = service
        .resolve_lorebook_conflict(ResolveCharacterLorebookConflictDto {
            name: "Alice".to_string(),
            resolution: CharacterLorebookConflictResolution::Current,
            conflict_token: None,
        })
        .await
        .expect("resolve with current world");
    assert_eq!(current_result.world, "CurrentLore");
    assert_eq!(current_result.affected_world, None);
    assert!(!current_result.world_written);
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

    let embedded_conflict = service
        .check_lorebook_conflict(CheckCharacterLorebookConflictDto {
            name: "Bob".to_string(),
        })
        .await
        .expect("check embedded conflict");
    let embedded_result = service
        .resolve_lorebook_conflict(ResolveCharacterLorebookConflictDto {
            name: "Bob".to_string(),
            resolution: CharacterLorebookConflictResolution::Embedded,
            conflict_token: Some(
                embedded_conflict
                    .conflict_token
                    .expect("embedded conflict token"),
            ),
        })
        .await
        .expect("resolve with embedded book");
    assert_eq!(embedded_result.world, "EmbeddedLore");
    assert_eq!(
        embedded_result.affected_world.as_deref(),
        Some("EmbeddedLore")
    );
    assert!(embedded_result.world_written);
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
async fn character_service_embedded_resolution_overwrites_the_linked_world() {
    let root = temp_root("character-embedded-lorebook-link");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("CurrentLore", &world_info("current lore"))
        .await
        .expect("save current world info");
    let mut card = character_card("Alice", json!({ "world": "CurrentLore" }));
    card["data"]["character_book"] = character_book("UpdatedLore", "updated lore");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write conflicting character");

    let conflict = service
        .check_lorebook_conflict(CheckCharacterLorebookConflictDto {
            name: "Alice".to_string(),
        })
        .await
        .expect("check lorebook conflict");
    let resolved = service
        .resolve_lorebook_conflict(ResolveCharacterLorebookConflictDto {
            name: "Alice".to_string(),
            resolution: CharacterLorebookConflictResolution::Embedded,
            conflict_token: conflict.conflict_token,
        })
        .await
        .expect("resolve with embedded world");

    assert_eq!(resolved.world, "CurrentLore");
    assert_eq!(resolved.affected_world.as_deref(), Some("CurrentLore"));
    assert!(resolved.world_written);
    assert_eq!(
        read_stored_card(&root, "Alice")
            .await
            .pointer("/data/extensions/world"),
        Some(&json!("CurrentLore"))
    );
    assert_eq!(
        world_repository
            .get_world_info("CurrentLore", false)
            .await
            .expect("read current world")
            .expect("current world exists")
            .pointer("/entries/0/content"),
        Some(&json!("updated lore"))
    );
    assert!(
        world_repository
            .get_world_info("UpdatedLore", false)
            .await
            .expect("check unlinked embedded world")
            .is_none()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_rejects_stale_lorebook_resolution() {
    let root = temp_root("character-lorebook-stale-resolution");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("Lore", &world_info("local version one"))
        .await
        .expect("save initial world info");
    let mut card = character_card("Alice", json!({ "world": "Lore" }));
    card["data"]["character_book"] = character_book("Lore", "embedded update");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write conflicting character");

    let conflict = service
        .check_lorebook_conflict(CheckCharacterLorebookConflictDto {
            name: "Alice".to_string(),
        })
        .await
        .expect("check conflict");
    world_repository
        .save_world_info("Lore", &world_info("local version two"))
        .await
        .expect("edit world while choice is open");

    let error = service
        .resolve_lorebook_conflict(ResolveCharacterLorebookConflictDto {
            name: "Alice".to_string(),
            resolution: CharacterLorebookConflictResolution::Embedded,
            conflict_token: Some(conflict.conflict_token.expect("conflict token")),
        })
        .await
        .expect_err("stale choice must not overwrite the newer local world");
    assert!(matches!(error, ApplicationError::Conflict(_)));
    let current = world_repository
        .get_world_info("Lore", false)
        .await
        .expect("read current world")
        .expect("current world exists");
    assert_eq!(
        current.pointer("/entries/1/content"),
        Some(&json!("local version two"))
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

    let mut named_card = character_card("Named Import", json!({ "world": "Embedded Lore" }));
    named_card["data"]["character_book"] = character_book("Embedded Lore", "updated embedded lore");
    let named_import_path = root.join("named-import.png");
    fs::write(&named_import_path, character_png(&named_card))
        .await
        .expect("write named import png");

    let named_import = service
        .import_character(ImportCharacterDto {
            file_path: named_import_path.to_string_lossy().to_string(),
            preserve_file_name: Some("Named.png".to_string()),
        })
        .await
        .expect("import new character with a prescribed file name");
    assert_eq!(named_import.avatar, "Named.png");
    let named_stored_card = read_stored_card(&root, "Named").await;
    assert_eq!(
        named_stored_card.pointer("/data/extensions/world"),
        Some(&json!("Embedded Lore (1)"))
    );
    assert!(
        world_repository
            .get_world_info("Embedded Lore (1)", false)
            .await
            .expect("read named import world info")
            .is_some()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_import_keeps_unrecognized_embedded_lorebook_data() {
    let root = temp_root("character-import-open-lorebook");
    let service = character_service(&root).await;
    let mut card = character_card("Alice", json!({}));
    card["data"]["character_book"] = json!({ "entries": "not a supported entry set" });
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
        .expect("import card with opaque optional lorebook data");

    let stored = read_stored_card(&root, imported.avatar.trim_end_matches(".png")).await;
    assert_eq!(
        stored.pointer("/data/character_book/entries"),
        Some(&json!("not a supported entry set"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_import_keeps_character_when_embedded_lorebook_import_fails() {
    let root = temp_root("character-import-broken-lorebook");
    let service = character_service(&root).await;
    let mut card = character_card("Alice", json!({ "world": "" }));
    card["data"]["character_book"] = character_book("Broken Lore", "embedded lore");
    let import_path = root.join("import.png");
    fs::write(&import_path, character_png(&card))
        .await
        .expect("write import png");
    let worlds_dir = root.join("default-user/worlds");
    fs::create_dir_all(&worlds_dir)
        .await
        .expect("create worlds dir");
    fs::write(worlds_dir.join("Broken Lore.json"), b"not json")
        .await
        .expect("write broken world info");

    let imported = service
        .import_character(ImportCharacterDto {
            file_path: import_path.to_string_lossy().to_string(),
            preserve_file_name: None,
        })
        .await
        .expect("keep imported character when optional lorebook import fails");

    let stored = read_stored_card(&root, imported.avatar.trim_end_matches(".png")).await;
    assert_eq!(
        stored.pointer("/data/character_book/entries/0/content"),
        Some(&json!("embedded lore"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_replace_and_copy_preserve_local_lorebook_binding() {
    let root = temp_root("character-preserved-import-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    world_repository
        .save_world_info("Old Lore", &world_info("old linked lore"))
        .await
        .expect("save old world info");

    let mut old_card = character_card("Preserved", json!({ "world": "Old Lore" }));
    old_card["first_mes"] = json!("old first message");
    old_card["data"]["first_mes"] = json!("old first message");
    old_card["data"]["character_book"] = character_book("Old Lore", "old embedded lore");
    fs::write(
        root.join("default-user/characters/Preserved.png"),
        character_png(&old_card),
    )
    .await
    .expect("write old character card");

    let cached_old = service
        .get_character("Preserved")
        .await
        .expect("cache old character");
    assert_eq!(cached_old.first_mes, "old first message");

    let mut replacement_card =
        character_card("Imported Replacement", json!({ "world": "Incoming Lore" }));
    replacement_card["first_mes"] = json!("new first message");
    replacement_card["data"]["first_mes"] = json!("new first message");
    replacement_card["data"]["character_book"] =
        character_book("Incoming Lore", "new embedded lore");
    let import_path = root.join("replacement.png");
    fs::write(&import_path, character_png(&replacement_card))
        .await
        .expect("write replacement character card");

    let imported = service
        .replace_character(ReplaceCharacterDto {
            file_path: import_path.to_string_lossy().to_string(),
            name: "Preserved".to_string(),
        })
        .await
        .expect("replace character");
    assert_eq!(imported.avatar, "Preserved.png");
    assert_eq!(imported.name, "Imported Replacement");
    assert_eq!(imported.first_mes, "new first message");

    let reloaded = service
        .get_character("Preserved")
        .await
        .expect("reload replacement");
    assert_eq!(reloaded.name, "Imported Replacement");
    assert_eq!(reloaded.first_mes, "new first message");
    assert_eq!(
        reloaded
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.get("world")),
        Some(&json!("Old Lore"))
    );
    assert_eq!(
        reloaded
            .character_book
            .as_ref()
            .and_then(|book| book.pointer("/entries/0/content")),
        Some(&json!("new embedded lore"))
    );

    let stored_card = read_stored_card(&root, "Preserved").await;
    assert_eq!(
        stored_card.pointer("/data/extensions/world"),
        Some(&json!("Old Lore"))
    );
    assert_eq!(
        stored_card.pointer("/data/character_book/entries/0/content"),
        Some(&json!("new embedded lore"))
    );

    let current_world = world_repository
        .get_world_info("Old Lore", false)
        .await
        .expect("read current world info")
        .expect("current world info exists");
    assert_eq!(
        current_world.pointer("/entries/1/content"),
        Some(&json!("old linked lore"))
    );
    assert!(
        world_repository
            .get_world_info("Old Lore (1)", false)
            .await
            .expect("check implicit lorebook copy")
            .is_none()
    );

    let conflict = service
        .check_lorebook_conflict(CheckCharacterLorebookConflictDto {
            name: "Preserved".to_string(),
        })
        .await
        .expect("check deferred lorebook conflict");
    assert!(conflict.conflict);
    assert!(conflict.current_available);

    let resolved = service
        .resolve_lorebook_conflict(ResolveCharacterLorebookConflictDto {
            name: "Preserved".to_string(),
            resolution: CharacterLorebookConflictResolution::Copy,
            conflict_token: conflict.conflict_token,
        })
        .await
        .expect("keep both lorebooks after replacement");
    assert_eq!(resolved.world, "Old Lore");
    assert_eq!(resolved.affected_world.as_deref(), Some("Old Lore (1)"));
    let copy = world_repository
        .get_world_info("Old Lore (1)", false)
        .await
        .expect("read copied world")
        .expect("copy exists");
    assert_eq!(
        copy.pointer("/entries/0/content"),
        Some(&json!("new embedded lore"))
    );
    assert_eq!(
        world_repository
            .get_world_info("Old Lore", false)
            .await
            .expect("read original world"),
        Some(current_world)
    );
    assert_eq!(
        read_stored_card(&root, "Preserved")
            .await
            .pointer("/data/extensions/world"),
        Some(&json!("Old Lore"))
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_replaces_an_unreadable_existing_card() {
    let root = temp_root("character-replace-unreadable");
    let service = character_service(&root).await;
    fs::write(
        root.join("default-user/characters/Broken.png"),
        write_character_data_to_png(&minimal_png(), "{").expect("write invalid card metadata"),
    )
    .await
    .expect("write unreadable existing card");
    let replacement_path = root.join("replacement.png");
    fs::write(
        &replacement_path,
        character_png(&character_card("Recovered", json!({}))),
    )
    .await
    .expect("write replacement card");

    let replaced = service
        .replace_character(ReplaceCharacterDto {
            file_path: replacement_path.to_string_lossy().to_string(),
            name: "Broken".to_string(),
        })
        .await
        .expect("replace unreadable card");

    assert_eq!(replaced.avatar, "Broken.png");
    assert_eq!(replaced.name, "Recovered");

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_delete_removes_only_string_linked_lorebooks() {
    let root = temp_root("character-delete-lorebook");
    let (service, world_repository) = character_service_with_world_repository(&root).await;
    for name in ["Lore", "42"] {
        world_repository
            .save_world_info(name, &world_info(name))
            .await
            .expect("save world info");
    }
    for (name, world) in [("Linked", json!("Lore")), ("Opaque", json!(42))] {
        let card = character_card(name, json!({ "world": world }));
        fs::write(
            root.join(format!("default-user/characters/{name}.png")),
            character_png(&card),
        )
        .await
        .expect("write character card");
        service
            .delete_character(DeleteCharacterDto {
                name: name.to_string(),
                delete_chats: false,
            })
            .await
            .expect("delete character");
    }

    assert!(
        world_repository
            .get_world_info("Lore", false)
            .await
            .expect("read linked world")
            .is_none()
    );
    assert!(
        world_repository
            .get_world_info("42", false)
            .await
            .expect("read numeric world")
            .is_some()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_replace_rejects_non_segment_storage_identity() {
    let root = temp_root("character-replace-invalid-storage-identity");
    let service = character_service(&root).await;

    let error = service
        .replace_character(ReplaceCharacterDto {
            file_path: root.join("replacement.png").to_string_lossy().to_string(),
            name: "../outside".to_string(),
        })
        .await
        .expect_err("path-like replacement identity must fail before repository lookup");

    assert!(matches!(error, ApplicationError::ValidationError(_)));
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_single_merge_preserves_open_fields_and_uses_upstream_validation() {
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

    service
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
        .expect("V3 accepts open extension field types");
    assert_eq!(
        read_stored_card(&root, "Alice")
            .await
            .pointer("/data/extensions"),
        Some(&json!("not an object"))
    );

    let error = service
        .merge_character_card_data(
            "Alice",
            MergeCharacterCardDataDto {
                update: json!({
                    "description": "__@@UNSET@@__",
                    "data": "not an object"
                }),
            },
        )
        .await
        .expect_err("V3 still requires an object data container");
    assert!(matches!(error, ApplicationError::ValidationError(_)));

    let _ = fs::remove_dir_all(root).await;
}
