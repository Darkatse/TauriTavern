use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::Path;

use tt_domain::errors::DomainError;

use super::io_error;
use crate::extension_store_json;

fn json_error(path: &Path, error: serde_json::Error) -> DomainError {
    DomainError::InvalidData(format!(
        "Invalid extension store JSON {}: {error}",
        path.display()
    ))
}

pub(super) fn validate(source: &Path, target: &Path) -> Result<(), DomainError> {
    let reader = File::open(source)
        .map(BufReader::new)
        .map_err(|error| io_error("open JSON", source, error))?;
    extension_store_json::validate(reader).map_err(|error| json_error(target, error))
}

pub(super) fn merge(target: &Path, patch: &Path, output: &Path) -> Result<(), DomainError> {
    let patch = fs::read(patch).map_err(|error| io_error("read JSON patch", patch, error))?;
    extension_store_json::validate(patch.as_slice()).map_err(|error| {
        DomainError::InvalidData(format!(
            "Invalid extension store JSON patch for {}: {error}",
            target.display()
        ))
    })?;
    let entry = match fs::read(target) {
        Ok(entry) => Some(entry),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error("read JSON", target, error)),
    };
    let merged = extension_store_json::merge(entry.as_deref(), &patch)
        .map_err(|error| json_error(target, error))?;
    File::options()
        .write(true)
        .create_new(true)
        .open(output)
        .and_then(|mut file| file.write_all(&merged))
        .map_err(|error| io_error("write merged JSON", output, error))
}
