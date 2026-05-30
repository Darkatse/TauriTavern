use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use chrono::Utc;
use serde_json::{Value, json};
use tokio::fs as tokio_fs;
use uuid::Uuid;

use super::*;
use crate::domain::models::skill::{
    DEFAULT_SKILL_READ_FALLBACK_MAX_CHARS, SkillFileKind, SkillImportConflictKind,
    SkillImportInput, SkillInlineFile, SkillInstallAction, SkillInstallConflictStrategy,
    SkillInstallRequest, SkillMoveRequest, SkillReadRequest, SkillScope, SkillScopeFilter,
    SkillScopeRetargetRequest, SkillSearchRequest, SkillWriteRequest,
};
use crate::domain::repositories::skill_repository::SkillRepository;

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tauritavern-skill-{label}-{}",
        Uuid::new_v4().simple()
    ))
}

#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("read permissions for '{}': {}", path.display(), error))
        .permissions();
    permissions.set_mode(mode);
    std::fs::set_permissions(path, permissions)
        .unwrap_or_else(|error| panic!("set permissions for '{}': {}", path.display(), error));
}

fn global_scope() -> SkillScope {
    SkillScope::Global
}

fn global_filter() -> SkillScopeFilter {
    SkillScopeFilter::Global
}

fn profile_scope(id: &str) -> SkillScope {
    SkillScope::Profile {
        profile_id: id.to_string(),
    }
}

fn profile_filter(id: &str) -> SkillScopeFilter {
    SkillScopeFilter::Profile {
        profile_id: id.to_string(),
    }
}

fn preset_scope(api_id: &str, name: &str) -> SkillScope {
    SkillScope::Preset {
        api_id: api_id.to_string(),
        name: name.to_string(),
    }
}

fn preset_filter(api_id: &str, name: &str) -> SkillScopeFilter {
    SkillScopeFilter::Preset {
        api_id: api_id.to_string(),
        name: name.to_string(),
    }
}

fn character_scope(id: &str) -> SkillScope {
    SkillScope::Character {
        character_id: id.to_string(),
    }
}

fn character_filter(id: &str) -> SkillScopeFilter {
    SkillScopeFilter::Character {
        character_id: id.to_string(),
    }
}

fn inline_skill(name: &str, extra_files: Vec<(&str, &str)>) -> SkillImportInput {
    inline_skill_with_source(name, extra_files, json!({"kind": "test"}))
}

fn inline_skill_with_source(
    name: &str,
    extra_files: Vec<(&str, &str)>,
    source: Value,
) -> SkillImportInput {
    let mut files = vec![SkillInlineFile {
        path: "SKILL.md".to_string(),
        encoding: "utf8".to_string(),
        content: format!(
            "---\nname: {name}\ndescription: Use for testing Skill imports.\nmetadata:\n  tags:\n    - tests\n---\n\n# Test\n"
        ),
        media_type: None,
        size_bytes: None,
        sha256: None,
    }];
    files.extend(
        extra_files
            .into_iter()
            .map(|(path, content)| SkillInlineFile {
                path: path.to_string(),
                encoding: "utf8".to_string(),
                content: content.to_string(),
                media_type: None,
                size_bytes: None,
                sha256: None,
            }),
    );
    SkillImportInput::InlineFiles { files, source }
}

#[tokio::test]
async fn installs_inline_skill_and_reads_file() {
    let root = temp_root("install");
    let repository = FileSkillRepository::new(root.clone());
    let result = repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    assert_eq!(result.action, SkillInstallAction::Installed);
    let listed = repository
        .list_skills(global_filter())
        .await
        .expect("list skills");
    assert_eq!(listed[0].name, "test-skill");
    assert_eq!(listed[0].tags, vec!["tests"]);
    let files = repository
        .list_skill_files(global_scope(), "test-skill")
        .await
        .expect("list skill files");
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["SKILL.md", "references/a.md"]
    );
    assert_eq!(files[1].kind, SkillFileKind::Text);

    let read = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read skill file");
    assert_eq!(read.content, "hello");
    assert_eq!(read.chars, 5);
    assert_eq!(read.words, 1);
    assert_eq!(read.total_chars, 5);
    assert_eq!(read.total_words, 1);
    assert_eq!(read.resource_ref, "skills/test-skill/references/a.md");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn reads_skill_file_ranges() {
    let root = temp_root("read-ranges");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill(
                "test-skill",
                vec![("references/a.md", "alpha\nblue lantern\nomega")],
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let line = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: Some(2),
            line_count: Some(1),
            start_char: None,
            max_chars: Some(80),
        })
        .await
        .expect("read line range");
    assert_eq!(line.content, "blue lantern");
    assert_eq!(line.chars, 12);
    assert_eq!(line.words, 2);
    assert_eq!(line.total_words, 4);
    assert_eq!(line.start_line, 2);
    assert_eq!(line.end_line, 2);
    assert!(line.truncated);

    let chars = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: Some(6),
            max_chars: Some(4),
        })
        .await
        .expect("read char range");
    assert_eq!(chars.content, "blue");
    assert_eq!(chars.chars, 4);
    assert_eq!(chars.words, 1);
    assert_eq!(chars.total_words, 4);
    assert_eq!(chars.start_char, 6);
    assert_eq!(chars.end_char, 10);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn reads_default_budget_without_enforcing_it_as_a_hard_cap() {
    let root = temp_root("read-budget");
    let repository = FileSkillRepository::new(root.clone());
    let long_content = "a".repeat(120_000);
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/long.md", &long_content)]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let default_read = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/long.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read default range");
    assert_eq!(default_read.chars, DEFAULT_SKILL_READ_FALLBACK_MAX_CHARS);
    assert!(default_read.truncated);

    let profile_sized_read = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/long.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: Some(100_000),
        })
        .await
        .expect("read profile-sized range");
    assert_eq!(profile_sized_read.chars, 100_000);
    assert!(profile_sized_read.truncated);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn writes_skill_file_and_updates_index_metadata() {
    let root = temp_root("write-file");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");
    let before = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read before write");
    let before_index = repository
        .list_skills(global_filter())
        .await
        .expect("list before write");

    let saved = repository
        .write_skill_file(SkillWriteRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            content: "hello updated".to_string(),
            expected_sha256: Some(before.sha256),
        })
        .await
        .expect("write skill file");

    assert_eq!(saved.content, "hello updated");
    let listed = repository
        .list_skills(global_filter())
        .await
        .expect("list skills");
    assert_eq!(listed[0].total_bytes, before_index[0].total_bytes + 8);
    assert_ne!(listed[0].installed_hash, before_index[0].installed_hash);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn write_skill_file_rejects_stale_expected_hash() {
    let root = temp_root("write-stale");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let error = repository
        .write_skill_file(SkillWriteRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            content: "hello updated".to_string(),
            expected_sha256: Some("stale".to_string()),
        })
        .await
        .expect_err("stale hash should fail");
    assert!(error.to_string().contains("Skill file changed on disk"));

    let read = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read after rejected write");
    assert_eq!(read.content, "hello");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn write_skill_file_rejects_skill_rename() {
    let root = temp_root("write-rename");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let error = repository
        .write_skill_file(SkillWriteRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "SKILL.md".to_string(),
            content: "---\nname: other-skill\ndescription: Different name.\n---\n".to_string(),
            expected_sha256: None,
        })
        .await
        .expect_err("renaming through write should fail");
    assert!(error.to_string().contains("cannot rename"));

    let read = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "SKILL.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read after rejected rename");
    assert!(read.content.contains("name: test-skill"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn searches_installed_skill_text_files() {
    let root = temp_root("search");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill(
                "test-skill",
                vec![("references/a.md", "alpha\nblue lantern\nomega")],
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let search = repository
        .search_skill_files(SkillSearchRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            query: "blue lantern".to_string(),
            path: Some("references".to_string()),
            limit: 5,
            context_lines: 0,
        })
        .await
        .expect("search skill");
    assert_eq!(search.searched_files, 1);
    assert_eq!(search.hits[0].path, "references/a.md");
    assert_eq!(search.hits[0].start_line, 2);
    assert!(search.hits[0].snippet.contains("blue lantern"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn requires_explicit_replace_on_conflict() {
    let root = temp_root("conflict");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "one")]),
            conflict_strategy: None,
        })
        .await
        .expect("install initial");

    let error = repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "two")]),
            conflict_strategy: None,
        })
        .await
        .expect_err("conflict should fail");
    assert!(error.to_string().contains("conflict_strategy is required"));

    let replaced = repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "two")]),
            conflict_strategy: Some(SkillInstallConflictStrategy::Replace),
        })
        .await
        .expect("replace skill");
    assert_eq!(replaced.action, SkillInstallAction::Replaced);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn allows_same_skill_name_in_different_scopes() {
    let root = temp_root("scoped-same-name");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "global")]),
            conflict_strategy: None,
        })
        .await
        .expect("install global skill");
    repository
        .install_import(SkillInstallRequest {
            target_scope: profile_scope("writer"),
            input: inline_skill("test-skill", vec![("references/a.md", "profile")]),
            conflict_strategy: None,
        })
        .await
        .expect("install profile skill");

    let global = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read global skill");
    let profile = repository
        .read_skill_file(SkillReadRequest {
            scope: profile_scope("writer"),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read profile skill");

    assert_eq!(global.content, "global");
    assert_eq!(profile.content, "profile");
    assert_eq!(
        repository
            .list_skills(global_filter())
            .await
            .expect("global")
            .len(),
        1
    );
    assert_eq!(
        repository
            .list_skills(profile_filter("writer"))
            .await
            .expect("profile")
            .len(),
        1
    );
    assert_eq!(
        repository
            .list_skills(SkillScopeFilter::All)
            .await
            .expect("all")
            .len(),
        2
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn moves_skill_between_scopes() {
    let root = temp_root("move-scope");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "moved")]),
            conflict_strategy: None,
        })
        .await
        .expect("install global skill");

    let moved = repository
        .move_skill(SkillMoveRequest {
            name: "test-skill".to_string(),
            from_scope: global_scope(),
            to_scope: profile_scope("writer"),
            conflict_strategy: None,
        })
        .await
        .expect("move skill");

    assert_eq!(moved.action, SkillInstallAction::Installed);
    assert!(
        repository
            .list_skills(global_filter())
            .await
            .expect("global")
            .is_empty()
    );
    assert_eq!(
        repository
            .list_skills(profile_filter("writer"))
            .await
            .expect("profile")[0]
            .name,
        "test-skill"
    );
    let read = repository
        .read_skill_file(SkillReadRequest {
            scope: profile_scope("writer"),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read moved skill");
    assert_eq!(read.content, "moved");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn install_rolls_back_target_when_index_save_fails() {
    let root = temp_root("install-save-fail");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .list_skills(global_filter())
        .await
        .expect("initialize index");

    set_dir_mode(&root.join("index"), 0o555);
    let error = repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect_err("index save should fail");
    set_dir_mode(&root.join("index"), 0o755);

    assert!(
        error
            .to_string()
            .contains("Failed to write temporary Skill index")
    );
    assert!(
        repository
            .list_skills(global_filter())
            .await
            .expect("list")
            .is_empty()
    );
    assert!(
        !repository
            .installed_scope_root(&global_scope())
            .expect("global root")
            .join("test-skill")
            .exists()
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn delete_skill_keeps_directory_when_index_save_fails() {
    let root = temp_root("delete-save-fail");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");
    let skill_root = repository
        .installed_scope_root(&global_scope())
        .expect("global root")
        .join("test-skill");

    set_dir_mode(&root.join("index"), 0o555);
    let error = repository
        .delete_skill(global_scope(), "test-skill")
        .await
        .expect_err("index save should fail");
    set_dir_mode(&root.join("index"), 0o755);

    assert!(
        error
            .to_string()
            .contains("Failed to write temporary Skill index")
    );
    assert!(skill_root.exists());
    assert_eq!(
        repository
            .list_skills(global_filter())
            .await
            .expect("list")
            .len(),
        1
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn move_skill_rolls_back_target_copy_when_index_save_fails() {
    let root = temp_root("move-save-fail");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "moved")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");
    let source_root = repository
        .installed_scope_root(&global_scope())
        .expect("global root")
        .join("test-skill");
    let target_root = repository
        .installed_scope_root(&profile_scope("writer"))
        .expect("profile root")
        .join("test-skill");

    set_dir_mode(&root.join("index"), 0o555);
    let error = repository
        .move_skill(SkillMoveRequest {
            name: "test-skill".to_string(),
            from_scope: global_scope(),
            to_scope: profile_scope("writer"),
            conflict_strategy: None,
        })
        .await
        .expect_err("index save should fail");
    set_dir_mode(&root.join("index"), 0o755);

    assert!(
        error
            .to_string()
            .contains("Failed to write temporary Skill index")
    );
    assert!(source_root.exists());
    assert!(!target_root.exists());
    assert_eq!(
        repository
            .list_skills(global_filter())
            .await
            .expect("global")
            .len(),
        1
    );
    assert!(
        repository
            .list_skills(profile_filter("writer"))
            .await
            .expect("profile")
            .is_empty()
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn move_replace_rolls_back_target_when_index_save_fails() {
    let root = temp_root("move-replace-save-fail");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "source")]),
            conflict_strategy: None,
        })
        .await
        .expect("install source skill");
    repository
        .install_import(SkillInstallRequest {
            target_scope: profile_scope("writer"),
            input: inline_skill("test-skill", vec![("references/a.md", "target")]),
            conflict_strategy: None,
        })
        .await
        .expect("install target skill");

    set_dir_mode(&root.join("index"), 0o555);
    let error = repository
        .move_skill(SkillMoveRequest {
            name: "test-skill".to_string(),
            from_scope: global_scope(),
            to_scope: profile_scope("writer"),
            conflict_strategy: Some(SkillInstallConflictStrategy::Replace),
        })
        .await
        .expect_err("index save should fail");
    set_dir_mode(&root.join("index"), 0o755);

    assert!(
        error
            .to_string()
            .contains("Failed to write temporary Skill index")
    );
    let source = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read source");
    let target = repository
        .read_skill_file(SkillReadRequest {
            scope: profile_scope("writer"),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read target");
    assert_eq!(source.content, "source");
    assert_eq!(target.content, "target");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn retargets_preset_scope_and_source_refs() {
    let root = temp_root("retarget-preset");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: preset_scope("openai", "Old"),
            input: inline_skill_with_source(
                "preset-skill",
                vec![("references/a.md", "preset")],
                json!({"kind":"preset","id":"preset:openai:Old","label":"Old"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install preset skill");
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill_with_source(
                "global-linked",
                vec![],
                json!({"kind":"preset","id":"preset:openai:Old","label":"Old"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install globally linked skill");

    let result = repository
        .retarget_scope(SkillScopeRetargetRequest {
            from_scope: preset_scope("openai", "Old"),
            to_scope: preset_scope("openai", "New"),
        })
        .await
        .expect("retarget preset");

    assert_eq!(result.moved, 1);
    assert_eq!(result.merged, 0);
    assert_eq!(result.source_refs_updated, 2);
    assert!(
        repository
            .list_skills(preset_filter("openai", "Old"))
            .await
            .expect("old scope")
            .is_empty()
    );
    let preset_skills = repository
        .list_skills(preset_filter("openai", "New"))
        .await
        .expect("new scope");
    assert_eq!(preset_skills.len(), 1);
    assert_eq!(preset_skills[0].source_refs[0].id, "preset:openai:New");
    assert_eq!(preset_skills[0].source_refs[0].label, "New");

    let global_skills = repository
        .list_skills(global_filter())
        .await
        .expect("global");
    assert_eq!(global_skills.len(), 1);
    assert_eq!(global_skills[0].source_refs[0].id, "preset:openai:New");
    assert_eq!(global_skills[0].source_refs[0].label, "New");

    let deleted = repository
        .delete_skills_for_source("preset", "preset:openai:Old")
        .await
        .expect("delete old source");
    assert!(deleted.is_empty());

    let read = repository
        .read_skill_file(SkillReadRequest {
            scope: preset_scope("openai", "New"),
            name: "preset-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect("read retargeted skill");
    assert_eq!(read.content, "preset");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn retarget_rejects_different_target_content() {
    let root = temp_root("retarget-conflict");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: preset_scope("openai", "Old"),
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "old")],
                json!({"kind":"preset","id":"preset:openai:Old","label":"Old"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install old skill");
    repository
        .install_import(SkillInstallRequest {
            target_scope: preset_scope("openai", "New"),
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "new")],
                json!({"kind":"preset","id":"preset:openai:New","label":"New"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install new skill");

    let error = repository
        .retarget_scope(SkillScopeRetargetRequest {
            from_scope: preset_scope("openai", "Old"),
            to_scope: preset_scope("openai", "New"),
        })
        .await
        .expect_err("different target content must fail");
    assert!(
        error
            .to_string()
            .contains("already exists in target scope with different content")
    );
    assert_eq!(
        repository
            .list_skills(preset_filter("openai", "Old"))
            .await
            .expect("old scope")
            .len(),
        1
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn retarget_rolls_back_prepared_target_when_index_save_fails() {
    let root = temp_root("retarget-save-fail");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: preset_scope("openai", "Old"),
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "old")],
                json!({"kind":"preset","id":"preset:openai:Old","label":"Old"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install old skill");
    let old_root = repository
        .installed_scope_root(&preset_scope("openai", "Old"))
        .expect("old root")
        .join("test-skill");
    let new_root = repository
        .installed_scope_root(&preset_scope("openai", "New"))
        .expect("new root")
        .join("test-skill");

    set_dir_mode(&root.join("index"), 0o555);
    let error = repository
        .retarget_scope(SkillScopeRetargetRequest {
            from_scope: preset_scope("openai", "Old"),
            to_scope: preset_scope("openai", "New"),
        })
        .await
        .expect_err("index save should fail");
    set_dir_mode(&root.join("index"), 0o755);

    assert!(
        error
            .to_string()
            .contains("Failed to write temporary Skill index")
    );
    assert!(old_root.exists());
    assert!(!new_root.exists());
    assert_eq!(
        repository
            .list_skills(preset_filter("openai", "Old"))
            .await
            .expect("old scope")
            .len(),
        1
    );
    assert!(
        repository
            .list_skills(preset_filter("openai", "New"))
            .await
            .expect("new scope")
            .is_empty()
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn retarget_reports_cleanup_failure_after_index_commit() {
    let root = temp_root("retarget-cleanup-fail");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: preset_scope("openai", "Old"),
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "old")],
                json!({"kind":"preset","id":"preset:openai:Old","label":"Old"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install old skill");
    let old_scope_root = repository
        .installed_scope_root(&preset_scope("openai", "Old"))
        .expect("old root");

    set_dir_mode(&old_scope_root, 0o555);
    let error = repository
        .retarget_scope(SkillScopeRetargetRequest {
            from_scope: preset_scope("openai", "Old"),
            to_scope: preset_scope("openai", "New"),
        })
        .await
        .expect_err("source cleanup should fail after commit");
    set_dir_mode(&old_scope_root, 0o755);

    assert!(
        error
            .to_string()
            .contains("retarget_skill_scope committed but failed to clean up Skill directories")
    );
    assert!(
        repository
            .list_skills(preset_filter("openai", "Old"))
            .await
            .expect("old scope")
            .is_empty()
    );
    assert_eq!(
        repository
            .list_skills(preset_filter("openai", "New"))
            .await
            .expect("new scope")
            .len(),
        1
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn retargets_character_scope() {
    let root = temp_root("retarget-character");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: character_scope("Aurelia"),
            input: inline_skill_with_source(
                "character-skill",
                vec![],
                json!({"kind":"character","id":"character:Aurelia","label":"Aurelia"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install character skill");

    let result = repository
        .retarget_scope(SkillScopeRetargetRequest {
            from_scope: character_scope("Aurelia"),
            to_scope: character_scope("Aurelia_Renamed"),
        })
        .await
        .expect("retarget character");

    assert_eq!(result.moved, 1);
    assert_eq!(result.source_refs_updated, 1);
    assert!(
        repository
            .list_skills(character_filter("Aurelia"))
            .await
            .expect("old character")
            .is_empty()
    );
    let listed = repository
        .list_skills(character_filter("Aurelia_Renamed"))
        .await
        .expect("new character");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source_refs[0].id, "character:Aurelia_Renamed");
    assert_eq!(listed[0].source_refs[0].label, "Aurelia_Renamed");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn list_rejects_invalid_scope_filter() {
    let root = temp_root("invalid-scope-filter");
    let repository = FileSkillRepository::new(root.clone());
    let error = repository
        .list_skills(SkillScopeFilter::Profile {
            profile_id: "Writer".to_string(),
        })
        .await
        .expect_err("invalid scope filter should fail");

    assert!(error.to_string().contains("profile id must use lowercase"));

    let _ = tokio_fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn deletes_skill_when_last_linked_source_is_deleted() {
    let root = temp_root("delete-source-last");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill_with_source(
                "test-skill",
                vec![],
                json!({"kind":"preset","id":"preset:openai:One","label":"One"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let deleted = repository
        .delete_skills_for_source("preset", "preset:openai:One")
        .await
        .expect("delete linked skills");
    assert_eq!(deleted, vec!["global/test-skill"]);
    assert!(
        repository
            .list_skills(global_filter())
            .await
            .expect("list")
            .is_empty()
    );
    assert!(
        !root
            .join("installed")
            .join("global")
            .join("test-skill")
            .exists()
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn delete_skills_for_source_keeps_directory_when_index_save_fails() {
    let root = temp_root("delete-source-save-fail");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill_with_source(
                "test-skill",
                vec![],
                json!({"kind":"preset","id":"preset:openai:One","label":"One"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install linked skill");
    let skill_root = repository
        .installed_scope_root(&global_scope())
        .expect("global root")
        .join("test-skill");

    set_dir_mode(&root.join("index"), 0o555);
    let error = repository
        .delete_skills_for_source("preset", "preset:openai:One")
        .await
        .expect_err("index save should fail");
    set_dir_mode(&root.join("index"), 0o755);

    assert!(
        error
            .to_string()
            .contains("Failed to write temporary Skill index")
    );
    assert!(skill_root.exists());
    assert_eq!(
        repository
            .list_skills(global_filter())
            .await
            .expect("list")
            .len(),
        1
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn deletes_selected_skill() {
    let root = temp_root("delete-selected");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    repository
        .delete_skill(global_scope(), "test-skill")
        .await
        .expect("delete selected skill");

    assert!(
        repository
            .list_skills(global_filter())
            .await
            .expect("list")
            .is_empty()
    );
    assert!(
        !root
            .join("installed")
            .join("global")
            .join("test-skill")
            .exists()
    );
    let error = repository
        .delete_skill(global_scope(), "test-skill")
        .await
        .expect_err("missing skill should fail");
    assert!(
        error
            .to_string()
            .contains("Skill not found: global/test-skill")
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn keeps_skill_until_all_linked_sources_are_deleted() {
    let root = temp_root("delete-source-shared");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill_with_source(
                "test-skill",
                vec![],
                json!({"kind":"preset","id":"preset:openai:One","label":"One"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install first source");
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill_with_source(
                "test-skill",
                vec![],
                json!({"kind":"character","id":"character:Aurelia","label":"Aurelia"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("link same skill to second source");

    let deleted = repository
        .delete_skills_for_source("preset", "preset:openai:One")
        .await
        .expect("delete first source");
    assert!(deleted.is_empty());
    let listed = repository.list_skills(global_filter()).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source_refs.len(), 1);
    assert_eq!(listed[0].source_refs[0].id, "character:Aurelia");

    let deleted = repository
        .delete_skills_for_source("character", "character:Aurelia")
        .await
        .expect("delete second source");
    assert_eq!(deleted, vec!["global/test-skill"]);
    assert!(
        repository
            .list_skills(global_filter())
            .await
            .expect("list")
            .is_empty()
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn replacing_skill_drops_previous_source_links() {
    let root = temp_root("replace-source-links");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "one")],
                json!({"kind":"preset","id":"preset:openai:One","label":"One"}),
            ),
            conflict_strategy: None,
        })
        .await
        .expect("install first source");
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill_with_source(
                "test-skill",
                vec![("references/a.md", "two")],
                json!({"kind":"character","id":"character:Aurelia","label":"Aurelia"}),
            ),
            conflict_strategy: Some(SkillInstallConflictStrategy::Replace),
        })
        .await
        .expect("replace skill");

    let deleted = repository
        .delete_skills_for_source("preset", "preset:openai:One")
        .await
        .expect("delete old source");
    assert!(deleted.is_empty());
    let listed = repository.list_skills(global_filter()).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].source_refs.len(), 1);
    assert_eq!(listed[0].source_refs[0].id, "character:Aurelia");

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn rejects_invalid_sidecar_schema() {
    let root = temp_root("sidecar");
    let repository = FileSkillRepository::new(root.clone());
    let error = repository
        .preview_import(
            inline_skill(
                "test-skill",
                vec![(
                    "agents/tauritavern.json",
                    r#"{"version":1,"unexpected":true}"#,
                )],
            ),
            global_scope(),
        )
        .await
        .expect_err("invalid sidecar should fail");
    assert!(
        error
            .to_string()
            .contains("Invalid agents/tauritavern.json")
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn exported_skill_archive_can_be_reimported() {
    let root = temp_root("export");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let exported = repository
        .export_skill(global_scope(), "test-skill")
        .await
        .expect("export skill");
    assert_eq!(exported.file_name, "test-skill.zip");
    let archive_path = root.join(&exported.file_name);
    tokio_fs::write(&archive_path, exported.bytes)
        .await
        .expect("write archive");

    let second_root = temp_root("reimport");
    let second_repository = FileSkillRepository::new(second_root.clone());
    let preview = second_repository
        .preview_import(
            SkillImportInput::ArchiveFile {
                path: archive_path.to_string_lossy().to_string(),
                source: json!({"kind": "test"}),
            },
            global_scope(),
        )
        .await
        .expect("preview exported archive");
    assert_eq!(preview.skill.name, "test-skill");
    assert_eq!(preview.conflict.kind, SkillImportConflictKind::New);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
    tokio_fs::remove_dir_all(second_root)
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn exported_skill_archive_base64_can_be_reimported() {
    let root = temp_root("export-base64");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let exported = repository
        .export_skill(global_scope(), "test-skill")
        .await
        .expect("export skill");
    assert_eq!(exported.file_name, "test-skill.zip");
    let second_root = temp_root("reimport-base64");
    let second_repository = FileSkillRepository::new(second_root.clone());
    let preview = second_repository
        .preview_import(
            SkillImportInput::ArchiveBase64 {
                file_name: exported.file_name,
                content_base64: BASE64_STANDARD.encode(exported.bytes),
                sha256: Some(exported.sha256),
                source: json!({"kind": "test"}),
            },
            global_scope(),
        )
        .await
        .expect("preview exported archive");
    assert_eq!(preview.skill.name, "test-skill");
    assert_eq!(preview.conflict.kind, SkillImportConflictKind::New);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
    tokio_fs::remove_dir_all(second_root)
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn exported_skill_roundtrip_preserves_hash() {
    let root = temp_root("export-same");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let installed_hash = repository
        .list_skills(global_filter())
        .await
        .expect("list skills")[0]
        .installed_hash
        .clone();
    let exported = repository
        .export_skill(global_scope(), "test-skill")
        .await
        .expect("export skill");
    assert_eq!(exported.file_name, "test-skill.zip");
    // Historical .ttskill files are still zip archives and must remain import-compatible.
    let archive_path = root.join("test-skill.ttskill");
    tokio_fs::write(&archive_path, exported.bytes)
        .await
        .expect("write archive");

    let preview = repository
        .preview_import(
            SkillImportInput::ArchiveFile {
                path: archive_path.to_string_lossy().to_string(),
                source: json!({"kind": "test"}),
            },
            global_scope(),
        )
        .await
        .expect("preview exported archive");

    assert_eq!(preview.conflict.kind, SkillImportConflictKind::Same);
    assert_eq!(preview.skill.installed_hash, installed_hash);

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn migrates_v1_unscoped_index_to_global_scope() {
    let root = temp_root("migrate-v1");
    let skill_root = root.join("installed").join("legacy-skill");
    tokio_fs::create_dir_all(&skill_root)
        .await
        .expect("create legacy skill");
    tokio_fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: legacy-skill\ndescription: Legacy Skill.\n---\n\n# Legacy\n",
    )
    .await
    .expect("write legacy skill");
    tokio_fs::create_dir_all(root.join("index"))
        .await
        .expect("create index dir");
    tokio_fs::write(
        root.join("index").join("skills.json"),
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "skills": [{
                "name": "legacy-skill",
                "description": "Legacy Skill.",
                "installedHash": "legacy-hash",
                "fileCount": 1,
                "totalBytes": 1,
                "hasScripts": false,
                "hasBinary": false,
                "installedAt": Utc::now().to_rfc3339(),
                "sourceRefs": [],
            }],
        }))
        .expect("serialize v1 index"),
    )
    .await
    .expect("write v1 index");

    let repository = FileSkillRepository::new(root.clone());
    let listed = repository
        .list_skills(global_filter())
        .await
        .expect("migrate index");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].scope, global_scope());
    assert!(!root.join("installed").join("legacy-skill").exists());
    assert!(
        root.join("installed")
            .join("global")
            .join("legacy-skill")
            .exists()
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn migrate_v1_rolls_back_prepared_global_dirs_on_later_failure() {
    let root = temp_root("migrate-v1-rollback");
    let first_root = root.join("installed").join("legacy-one");
    tokio_fs::create_dir_all(&first_root)
        .await
        .expect("create first legacy skill");
    tokio_fs::write(
        first_root.join("SKILL.md"),
        "---\nname: legacy-one\ndescription: Legacy Skill.\n---\n\n# Legacy\n",
    )
    .await
    .expect("write first legacy skill");
    tokio_fs::create_dir_all(root.join("index"))
        .await
        .expect("create index dir");
    tokio_fs::write(
        root.join("index").join("skills.json"),
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "skills": [
                {
                    "name": "legacy-one",
                    "description": "Legacy Skill.",
                    "installedHash": "legacy-one-hash",
                    "fileCount": 1,
                    "totalBytes": 1,
                    "hasScripts": false,
                    "hasBinary": false,
                    "installedAt": Utc::now().to_rfc3339(),
                    "sourceRefs": [],
                },
                {
                    "name": "missing-legacy",
                    "description": "Missing Legacy Skill.",
                    "installedHash": "missing-hash",
                    "fileCount": 1,
                    "totalBytes": 1,
                    "hasScripts": false,
                    "hasBinary": false,
                    "installedAt": Utc::now().to_rfc3339(),
                    "sourceRefs": [],
                },
            ],
        }))
        .expect("serialize v1 index"),
    )
    .await
    .expect("write v1 index");

    let repository = FileSkillRepository::new(root.clone());
    let error = repository
        .list_skills(global_filter())
        .await
        .expect_err("missing second legacy directory should fail migration");

    assert!(
        error
            .to_string()
            .contains("Skill directory not found during v1 migration: missing-legacy")
    );
    assert!(root.join("installed").join("legacy-one").exists());
    assert!(
        !root
            .join("installed")
            .join("global")
            .join("legacy-one")
            .exists()
    );
    let index_text = tokio_fs::read_to_string(root.join("index").join("skills.json"))
        .await
        .expect("read index");
    assert!(index_text.contains("\"version\": 1"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn migrate_v1_accepts_previously_moved_global_dir() {
    let root = temp_root("migrate-v1-previously-moved");
    let skill_root = root.join("installed").join("global").join("legacy-skill");
    tokio_fs::create_dir_all(&skill_root)
        .await
        .expect("create moved global skill");
    tokio_fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: legacy-skill\ndescription: Legacy Skill.\n---\n\n# Legacy\n",
    )
    .await
    .expect("write moved global skill");
    tokio_fs::create_dir_all(root.join("index"))
        .await
        .expect("create index dir");
    tokio_fs::write(
        root.join("index").join("skills.json"),
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "skills": [{
                "name": "legacy-skill",
                "description": "Legacy Skill.",
                "installedHash": "legacy-hash",
                "fileCount": 1,
                "totalBytes": 1,
                "hasScripts": false,
                "hasBinary": false,
                "installedAt": Utc::now().to_rfc3339(),
                "sourceRefs": [],
            }],
        }))
        .expect("serialize v1 index"),
    )
    .await
    .expect("write v1 index");

    let repository = FileSkillRepository::new(root.clone());
    let listed = repository
        .list_skills(global_filter())
        .await
        .expect("finish partial migration");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].scope, global_scope());
    assert!(!root.join("installed").join("legacy-skill").exists());
    assert!(
        root.join("installed")
            .join("global")
            .join("legacy-skill")
            .exists()
    );
    let index_text = tokio_fs::read_to_string(root.join("index").join("skills.json"))
        .await
        .expect("read migrated index");
    assert!(index_text.contains("\"version\": 2"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[tokio::test]
async fn migrate_v1_rejects_target_inside_legacy_source() {
    let root = temp_root("migrate-v1-nested-target");
    let skill_root = root.join("installed").join("global");
    tokio_fs::create_dir_all(&skill_root)
        .await
        .expect("create legacy skill named global");
    tokio_fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: global\ndescription: Legacy Skill.\n---\n\n# Legacy\n",
    )
    .await
    .expect("write legacy skill");
    tokio_fs::create_dir_all(root.join("index"))
        .await
        .expect("create index dir");
    tokio_fs::write(
        root.join("index").join("skills.json"),
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "skills": [{
                "name": "global",
                "description": "Legacy Skill.",
                "installedHash": "legacy-hash",
                "fileCount": 1,
                "totalBytes": 1,
                "hasScripts": false,
                "hasBinary": false,
                "installedAt": Utc::now().to_rfc3339(),
                "sourceRefs": [],
            }],
        }))
        .expect("serialize v1 index"),
    )
    .await
    .expect("write v1 index");

    let repository = FileSkillRepository::new(root.clone());
    let error = repository
        .list_skills(global_filter())
        .await
        .expect_err("nested migration target should fail fast");

    assert!(
        error
            .to_string()
            .contains("Skill target directory cannot be inside source directory")
    );
    assert!(
        !root
            .join("installed")
            .join("global")
            .join("global")
            .exists()
    );

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}

#[cfg(unix)]
#[tokio::test]
async fn read_rejects_symlink_escape_inside_installed_skill() {
    use std::os::unix::fs::symlink;

    let root = temp_root("symlink-escape");
    let repository = FileSkillRepository::new(root.clone());
    repository
        .install_import(SkillInstallRequest {
            target_scope: global_scope(),
            input: inline_skill("test-skill", vec![("references/a.md", "hello")]),
            conflict_strategy: None,
        })
        .await
        .expect("install skill");

    let skill_root = root.join("installed").join("global").join("test-skill");
    let outside = root.join("outside");
    tokio_fs::create_dir_all(&outside)
        .await
        .expect("create outside dir");
    tokio_fs::write(outside.join("a.md"), "outside")
        .await
        .expect("write outside file");
    tokio_fs::remove_dir_all(skill_root.join("references"))
        .await
        .expect("remove references dir");
    symlink(&outside, skill_root.join("references")).expect("create symlink");

    let error = repository
        .read_skill_file(SkillReadRequest {
            scope: global_scope(),
            name: "test-skill".to_string(),
            path: "references/a.md".to_string(),
            start_line: None,
            line_count: None,
            start_char: None,
            max_chars: None,
        })
        .await
        .expect_err("symlink escape should fail");
    assert!(error.to_string().contains("escapes installed directory"));

    tokio_fs::remove_dir_all(root).await.expect("cleanup");
}
