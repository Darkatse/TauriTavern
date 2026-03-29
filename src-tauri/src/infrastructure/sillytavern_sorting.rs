use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use icu_collator::{CollatorBorrowed, options::CollatorOptions};
use icu_locale_core::Locale;

use crate::domain::errors::DomainError;

std::thread_local! {
    static SILLYTAVERN_NAME_COLLATOR: CollatorBorrowed<'static> = build_sillytavern_name_collator();
}

fn normalize_system_locale_tag(raw: &str) -> String {
    raw.split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-")
}

fn build_sillytavern_name_collator() -> CollatorBorrowed<'static> {
    let locale = sys_locale::get_locale()
        .map(|raw| normalize_system_locale_tag(&raw))
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<Locale>().ok())
        .unwrap_or(Locale::UNKNOWN);

    CollatorBorrowed::try_new(locale.into(), CollatorOptions::default())
        .expect("SillyTavern collator should initialize")
}

pub fn compare_js_default(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

pub fn compare_sillytavern_name(left: &str, right: &str) -> Ordering {
    SILLYTAVERN_NAME_COLLATOR.with(|collator| collator.compare(left, right))
}

#[cfg(test)]
pub fn sort_strings_js_default(values: &mut [String]) {
    values.sort_by(|left, right| compare_js_default(left, right));
}

pub fn sort_strings_sillytavern_name(values: &mut [String]) {
    values.sort_by(|left, right| compare_sillytavern_name(left, right));
}

pub fn sort_paths_by_file_name_js_default(paths: &mut [PathBuf]) -> Result<(), DomainError> {
    let mut sortable = paths
        .iter()
        .map(|path| path_file_name(path).map(|name| (name.to_string(), path.clone())))
        .collect::<Result<Vec<_>, _>>()?;
    sortable.sort_by(|(left_name, _), (right_name, _)| compare_js_default(left_name, right_name));
    paths
        .iter_mut()
        .zip(sortable)
        .for_each(|(slot, (_, path))| *slot = path);
    Ok(())
}

pub fn sort_paths_by_file_name_sillytavern_name(paths: &mut [PathBuf]) -> Result<(), DomainError> {
    let mut sortable = paths
        .iter()
        .map(|path| path_file_name(path).map(|name| (name.to_string(), path.clone())))
        .collect::<Result<Vec<_>, _>>()?;
    sortable
        .sort_by(|(left_name, _), (right_name, _)| compare_sillytavern_name(left_name, right_name));
    paths
        .iter_mut()
        .zip(sortable)
        .for_each(|(slot, (_, path))| *slot = path);
    Ok(())
}

fn path_file_name(path: &Path) -> Result<&str, DomainError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            DomainError::InvalidData(format!(
                "Path file name is not valid UTF-8: {}",
                path.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::{
        compare_js_default, compare_sillytavern_name, sort_paths_by_file_name_js_default,
        sort_paths_by_file_name_sillytavern_name, sort_strings_js_default,
        sort_strings_sillytavern_name,
    };
    use std::path::PathBuf;

    #[test]
    fn js_default_sort_matches_upstream_code_unit_order() {
        let mut values = vec![
            "😀Book".to_string(),
            "Abook".to_string(),
            "#Book".to_string(),
            "🧠Lore".to_string(),
            "✨Lore".to_string(),
            "_Book".to_string(),
            "-Book".to_string(),
        ];

        sort_strings_js_default(&mut values);

        assert_eq!(
            values,
            vec![
                "#Book".to_string(),
                "-Book".to_string(),
                "Abook".to_string(),
                "_Book".to_string(),
                "✨Lore".to_string(),
                "😀Book".to_string(),
                "🧠Lore".to_string(),
            ]
        );
        assert_eq!(
            compare_js_default("#Book", "-Book"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn sillytavern_name_sort_matches_upstream_locale_compare_order() {
        let mut values = vec![
            "😀Book".to_string(),
            "Abook".to_string(),
            "#Book".to_string(),
            "🧠Lore".to_string(),
            "✨Lore".to_string(),
            "_Book".to_string(),
            "-Book".to_string(),
        ];

        sort_strings_sillytavern_name(&mut values);

        assert_eq!(
            values,
            vec![
                "_Book".to_string(),
                "-Book".to_string(),
                "#Book".to_string(),
                "✨Lore".to_string(),
                "🧠Lore".to_string(),
                "😀Book".to_string(),
                "Abook".to_string(),
            ]
        );
        assert_eq!(
            compare_sillytavern_name("_Book", "Abook"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn path_sort_helpers_use_file_name_semantics() {
        let mut default_paths = vec![
            PathBuf::from("/tmp/😀Book.json"),
            PathBuf::from("/tmp/Abook.json"),
            PathBuf::from("/tmp/#Book.json"),
        ];
        sort_paths_by_file_name_js_default(&mut default_paths).expect("default sort");
        assert_eq!(
            default_paths,
            vec![
                PathBuf::from("/tmp/#Book.json"),
                PathBuf::from("/tmp/Abook.json"),
                PathBuf::from("/tmp/😀Book.json"),
            ]
        );

        let mut locale_paths = vec![
            PathBuf::from("/tmp/😀Book.json"),
            PathBuf::from("/tmp/Abook.json"),
            PathBuf::from("/tmp/#Book.json"),
        ];
        sort_paths_by_file_name_sillytavern_name(&mut locale_paths).expect("locale sort");
        assert_eq!(
            locale_paths,
            vec![
                PathBuf::from("/tmp/#Book.json"),
                PathBuf::from("/tmp/😀Book.json"),
                PathBuf::from("/tmp/Abook.json"),
            ]
        );
    }
}
