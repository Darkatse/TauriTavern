use std::io::Write;

use crate::infrastructure::persistence::data_archive_adapters::prepare_imported_user_data;
use tt_adapter_archive::FileDataArchiveExecutor;
use tt_ports::data_archive::{DataArchiveExecutor, ImportArchiveExecutionRequest};

#[test]
fn an_archive_whose_settings_cannot_be_split_leaves_the_target_as_it_was() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let data_root = temp.path().join("data");
    let settings_path = data_root.join("default-user/settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    std::fs::write(&settings_path, br#"{"marker":"old"}"#).unwrap();
    let workspace_root = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace_root).unwrap();
    let archive_path = temp.path().join("import.zip");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
    archive
        .start_file("settings.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    // Splitting opens accountStorage, whose key Rust cannot decode.
    archive
        .write_all(br#"{"accountStorage":{"\ud800":1}}"#)
        .unwrap();
    archive.finish().unwrap();
    let executor = FileDataArchiveExecutor {
        prepare_user_data: prepare_imported_user_data,
        read_personas: tt_adapter_media::persona_cards::read_personas,
    };

    let imported = executor.import_full_data(
        ImportArchiveExecutionRequest {
            data_root,
            archive_path,
            workspace_root,
        },
        &mut |_, _, _| {},
        &|| false,
    );

    assert!(imported.is_err());
    assert_eq!(
        std::fs::read(settings_path).unwrap(),
        br#"{"marker":"old"}"#
    );
}
