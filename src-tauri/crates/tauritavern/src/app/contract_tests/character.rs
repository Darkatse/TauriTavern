use super::*;

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
            "custom": "kept",
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
    assert_eq!(
        exported_card.pointer("/unknownTop/kept"),
        Some(&json!(true))
    );
    assert_eq!(
        exported_card.pointer("/data/extensions/custom"),
        Some(&json!("kept"))
    );
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

    let exported_json = service
        .export_character_content(ExportCharacterContentDto {
            name: "Alice".to_string(),
            format: "json".to_string(),
        })
        .await
        .expect("export json");
    let exported_file: Value =
        serde_json::from_slice(&exported_json.data).expect("parse exported json");
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
async fn character_service_replacement_adopts_the_new_lorebook_and_keeps_local_edits() {
    let root = temp_root("character-replacement-adoption");
    let (service, worlds) = character_service_with_world_repository(&root).await;
    let local = world_info("local edits");
    worlds
        .save_world_info("Lore v1", &local)
        .await
        .expect("save local lorebook");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&character_card("Alice", json!({ "world": "Lore v1" }))),
    )
    .await
    .expect("write existing character");
    service
        .get_character("Alice")
        .await
        .expect("cache old character");
    let mut incoming = character_card("Updated Alice", json!({ "world": "Lore v2" }));
    incoming["data"]["character_book"] = character_book("Lore v2", "new version");
    let source = root.join("incoming.png");
    fs::write(&source, character_png(&incoming))
        .await
        .expect("write incoming card");

    let imported = service
        .replace_character(ReplaceCharacterDto {
            name: "Alice".into(),
            file_path: source.to_string_lossy().into(),
        })
        .await
        .expect("replace character");
    assert_eq!(imported.character.name, "Updated Alice");
    assert_eq!(imported.character.avatar, "Alice.png");
    let CharacterLorebookAdoptionDto::Decision {
        world,
        target,
        can_update,
        token,
    } = imported.lorebook
    else {
        panic!("replacing different local content requires a decision");
    };
    assert_eq!(world, "Lore v1");
    assert_eq!(target, "Lore v2");
    assert!(!can_update);
    assert_eq!(
        read_stored_card(&root, "Alice")
            .await
            .pointer("/data/extensions/world"),
        Some(&json!("Lore v1"))
    );
    assert!(
        worlds
            .get_world_info("Lore v2")
            .await
            .expect("read proposed lorebook")
            .is_none()
    );

    let result = service
        .resolve_lorebook_adoption(ResolveCharacterLorebookAdoptionDto {
            name: "Alice".into(),
            choice: CharacterLorebookAdoptionChoice::Adopt,
            token,
        })
        .await
        .expect("adopt incoming lorebook");
    assert!(
        matches!(result, CharacterLorebookAdoptionDto::Linked { world, created: true, .. } if world == "Lore v2")
    );
    assert_eq!(
        read_stored_card(&root, "Alice")
            .await
            .pointer("/data/extensions/world"),
        Some(&json!("Lore v2"))
    );
    assert_eq!(
        worlds
            .get_world_info("Lore v1")
            .await
            .expect("read local lorebook"),
        Some(local)
    );
    assert_eq!(
        worlds
            .get_world_info("Lore v2")
            .await
            .expect("read adopted lorebook")
            .expect("adopted lorebook exists")
            .pointer("/entries/0/content"),
        Some(&json!("new version"))
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_rejects_stale_lorebook_adoption() {
    let root = temp_root("character-lorebook-stale-decision");
    let (service, worlds) = character_service_with_world_repository(&root).await;
    worlds
        .save_world_info("Lore", &world_info("local version one"))
        .await
        .expect("save local lorebook");
    let mut card = character_card("Alice", json!({ "world": "Lore" }));
    card["data"]["character_book"] = character_book("Lore", "embedded update");
    fs::write(
        root.join("default-user/characters/Alice.png"),
        character_png(&card),
    )
    .await
    .expect("write character card");
    let CharacterLorebookAdoptionDto::Decision { token, .. } = service
        .adopt_character_lorebook("Alice")
        .await
        .expect("prepare menu adoption")
    else {
        panic!("menu adoption must ask before changing local content");
    };
    let newer = world_info("local version two");
    worlds
        .save_world_info("Lore", &newer)
        .await
        .expect("edit lorebook while decision is pending");
    let error = service
        .resolve_lorebook_adoption(ResolveCharacterLorebookAdoptionDto {
            name: "Alice".into(),
            choice: CharacterLorebookAdoptionChoice::Update,
            token,
        })
        .await
        .expect_err("reject the stale decision");
    assert!(matches!(error, ApplicationError::Conflict(_)));
    assert_eq!(
        worlds
            .get_world_info("Lore")
            .await
            .expect("read newer local lorebook"),
        Some(newer)
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

    let stem = imported.character.avatar.trim_end_matches(".png");
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
        .get_world_info("Embedded Lore")
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
    assert_eq!(named_import.character.avatar, "Named.png");
    let named_stored_card = read_stored_card(&root, "Named").await;
    assert_eq!(
        named_stored_card.pointer("/data/extensions/world"),
        Some(&json!("Embedded Lore (1)"))
    );
    assert!(
        world_repository
            .get_world_info("Embedded Lore (1)")
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

    assert!(matches!(
        imported.lorebook,
        CharacterLorebookAdoptionDto::Failed
    ));
    let stem = imported.character.avatar.trim_end_matches(".png");
    let stored = read_stored_card(&root, stem).await;
    assert_eq!(
        stored.pointer("/data/character_book/entries"),
        Some(&json!("not a supported entry set"))
    );
    service
        .adopt_character_lorebook(stem)
        .await
        .expect_err("standalone adoption reports the invalid lorebook as an error");

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_updates_the_linked_copy_without_overwriting_the_base_book() {
    let root = temp_root("character-lorebook-copy-adoption");
    let (service, worlds) = character_service_with_world_repository(&root).await;
    let unrelated = world_info("another character's book");
    let local = world_info("local edits in the copy");
    worlds
        .save_world_info("Lore", &unrelated)
        .await
        .expect("save unrelated lorebook");
    let mut incoming = character_card("Updated", json!({ "world": "Lore" }));
    incoming["data"]["character_book"] = character_book("Lore", "incoming content");
    let source = root.join("incoming.png");
    fs::write(&source, character_png(&incoming))
        .await
        .expect("write incoming card");
    worlds
        .save_world_info("Lore (1)", &local)
        .await
        .expect("save current copy");
    fs::write(
        root.join("default-user/characters/Update.png"),
        character_png(&character_card("Update", json!({ "world": "Lore (1)" }))),
    )
    .await
    .expect("write character to update");
    let imported = service
        .replace_character(ReplaceCharacterDto {
            name: "Update".into(),
            file_path: source.to_string_lossy().into(),
        })
        .await
        .expect("replace character to update");
    let CharacterLorebookAdoptionDto::Decision {
        world,
        target,
        can_update,
        token,
    } = imported.lorebook
    else {
        panic!("copy update requires a decision");
    };
    assert_eq!(world, "Lore (1)");
    assert_eq!(target, "Lore (2)");
    assert!(can_update);
    service
        .resolve_lorebook_adoption(ResolveCharacterLorebookAdoptionDto {
            name: "Update".into(),
            choice: CharacterLorebookAdoptionChoice::Update,
            token,
        })
        .await
        .expect("update current copy");
    assert_eq!(
        read_stored_card(&root, "Update")
            .await
            .pointer("/data/extensions/world"),
        Some(&json!("Lore (1)"))
    );
    assert_eq!(
        worlds
            .get_world_info("Lore (1)")
            .await
            .expect("read updated copy")
            .expect("updated copy exists")
            .pointer("/entries/0/content"),
        Some(&json!("incoming content"))
    );

    assert_eq!(
        worlds
            .get_world_info("Lore")
            .await
            .expect("read unrelated lorebook"),
        Some(unrelated)
    );
    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn character_service_keeps_an_equal_copy_when_an_earlier_name_is_vacant() {
    let root = temp_root("character-lorebook-vacant-name");
    let (service, worlds) = character_service_with_world_repository(&root).await;
    let mut incoming = character_card("Seed", json!({}));
    incoming["data"]["character_book"] = character_book("Lore", "incoming content");
    let source = root.join("incoming.png");
    fs::write(&source, character_png(&incoming))
        .await
        .expect("write incoming card");
    service
        .import_character(ImportCharacterDto {
            file_path: source.to_string_lossy().into(),
            preserve_file_name: None,
        })
        .await
        .expect("import embedded lorebook");
    let adopted = worlds
        .get_world_info("Lore")
        .await
        .expect("read imported lorebook")
        .expect("imported lorebook exists");
    worlds
        .save_world_info("Lore (1)", &adopted)
        .await
        .expect("save identical copy");
    worlds
        .delete_world_info("Lore")
        .await
        .expect("vacate the earlier name");
    fs::write(
        root.join("default-user/characters/Gap.png"),
        character_png(&character_card("Gap", json!({ "world": "Lore (1)" }))),
    )
    .await
    .expect("write character using the copy");

    let imported = service
        .replace_character(ReplaceCharacterDto {
            name: "Gap".into(),
            file_path: source.to_string_lossy().into(),
        })
        .await
        .expect("replace character with the same lorebook content");
    assert!(
        matches!(imported.lorebook, CharacterLorebookAdoptionDto::Linked { world, created: false, updated: false, .. } if world == "Lore (1)")
    );
    assert!(
        worlds
            .get_world_info("Lore")
            .await
            .expect("check the earlier name remains vacant")
            .is_none()
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
        character_png(&character_card(
            "Recovered",
            json!({ "world": "OtherBook" }),
        )),
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

    assert_eq!(replaced.character.avatar, "Broken.png");
    assert_eq!(replaced.character.name, "Recovered");
    assert_eq!(
        read_stored_card(&root, "Broken")
            .await
            .pointer("/data/extensions/world"),
        Some(&json!(""))
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
