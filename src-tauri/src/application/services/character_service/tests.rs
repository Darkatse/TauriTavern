use serde_json::json;

use super::{CharacterService, card_contract};

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
