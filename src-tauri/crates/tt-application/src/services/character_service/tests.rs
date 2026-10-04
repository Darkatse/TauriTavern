use super::CharacterService;

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
fn invalid_bulk_merge_avatar_filename_fails_fast() {
    let error = CharacterService::normalize_merge_avatar_filename("../Alice.png").unwrap_err();

    assert!(error.to_string().contains("Invalid avatar filename"));
}
