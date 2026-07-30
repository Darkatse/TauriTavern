use serde_json::json;

use super::{CharacterService, card_contract};

#[test]
fn indexed_world_name_reserves_suffix_bytes() {
    let ascii = CharacterService::indexed_world_name(&"a".repeat(250), 2)
        .expect("allocate maximum-length ASCII name");
    assert!(ascii.ends_with(" (2)"));
    assert!(format!("{ascii}.json").len() <= 255);

    let unicode = CharacterService::indexed_world_name(&"中".repeat(83), 2)
        .expect("allocate maximum-length Unicode name");
    assert!(unicode.ends_with(" (2)"));
    assert!(format!("{unicode}.json").len() <= 255);
}

#[test]
fn indexed_world_copy_continues_existing_suffix_sequence() {
    assert_eq!(
        CharacterService::strip_trailing_index_suffix("Lore (2)"),
        "Lore"
    );
}

#[test]
fn export_contract_removes_private_fields_and_connection_refs() {
    let mut value = json!({
        "name": "Alice",
        "chat": "private-chat",
        "fav": true,
        "data": {
            "extensions": {
                "fav": true,
                "tauritavern": {
                    "agentProfiles": {
                        "version": 1,
                        "items": [{
                            "profile": {
                                "model": {
                                    "mode": "connectionRef",
                                    "connectionRef": "secret",
                                    "modelId": "private-model"
                                }
                            }
                        }]
                    }
                }
            }
        }
    });

    card_contract::unset_private_fields(&mut value).unwrap();
    card_contract::sanitize_agent_profiles_for_export(&mut value).unwrap();

    assert_eq!(value.get("chat"), None);
    assert_eq!(value.get("fav"), Some(&json!(false)));
    assert_eq!(value.pointer("/data/extensions/fav"), Some(&json!(false)));
    assert_eq!(
        value.pointer("/data/extensions/tauritavern/agentProfiles/items/0/profile/model"),
        Some(&json!({ "mode": "requiresConfiguration" }))
    );
}

#[test]
fn normalize_v2_character_book_adds_empty_extensions() {
    let mut value = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "data": {
            "character_book": {
                "name": "Lore",
                "entries": []
            }
        }
    });

    card_contract::normalize_v2_character_book_extensions(&mut value).unwrap();

    assert_eq!(
        value.pointer("/data/character_book/extensions"),
        Some(&json!({}))
    );
}

#[test]
fn normalize_v2_creator_metadata_projection_uses_data_as_canonical() {
    let mut value = json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "creator": "stale root creator",
        "creator_notes": "stale root notes",
        "character_version": "stale root version",
        "creatorcomment": "stale legacy notes",
        "data": {
            "creator": "",
            "creator_notes": "data notes",
            "character_version": "data version"
        }
    });

    card_contract::normalize_v2_creator_metadata_projection(&mut value).unwrap();

    assert_eq!(value.pointer("/creator"), Some(&json!("")));
    assert_eq!(value.pointer("/creator_notes"), Some(&json!("data notes")));
    assert_eq!(value.pointer("/creatorcomment"), Some(&json!("data notes")));
    assert_eq!(
        value.pointer("/character_version"),
        Some(&json!("data version"))
    );
}

#[test]
fn value_at_path_supports_bulk_filter_paths() {
    let value = json!({
        "data": {
            "tags": ["hero"],
            "extensions": {
                "world": "Lore"
            }
        }
    });

    assert_eq!(
        CharacterService::value_at_path(&value, "data.tags.0"),
        Some(&json!("hero"))
    );
    assert_eq!(
        CharacterService::value_at_path(&value, "data.extensions.world"),
        Some(&json!("Lore"))
    );
    assert!(CharacterService::value_at_path(&value, "data.tags.9").is_none());
}

#[test]
fn invalid_bulk_merge_avatar_filename_fails_fast() {
    let error = CharacterService::normalize_merge_avatar_filename("../Alice.png").unwrap_err();

    assert!(error.to_string().contains("Invalid avatar filename"));
}

#[test]
fn normalize_merge_avatar_filename_preserves_exact_identity() {
    assert_eq!(
        CharacterService::normalize_merge_avatar_filename(" Alice.png").unwrap(),
        " Alice.png"
    );
}
