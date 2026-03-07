use std::io::Read;
use std::path::PathBuf;

use zip::read::ZipFile;

use crate::domain::errors::DomainError;

pub fn enclosed_zip_entry_path<R: Read + ?Sized>(
    entry: &ZipFile<'_, R>,
) -> Result<PathBuf, DomainError> {
    entry
        .enclosed_name()
        .map(|path| path.to_path_buf())
        .ok_or_else(|| {
            DomainError::InvalidData(format!("Invalid archive entry path: {}", entry.name()))
        })
}
