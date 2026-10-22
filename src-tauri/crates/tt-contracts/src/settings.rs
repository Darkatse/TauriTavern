//! User settings files shared by storage and archive projection.

pub const CORE_FILE: &str = "settings.json";
pub const APPEARANCE_FILE: &str = "settings/appearance.json";
pub const PRESETS_FILE: &str = "settings/presets.json";
pub const LAYOUT_FILE: &str = "settings/layout.json";
pub const PERSONA_STATE_FILE: &str = "settings/persona-state.json";

pub const USER_SETTINGS_FILES: &[&str] = &[
    CORE_FILE,
    APPEARANCE_FILE,
    PRESETS_FILE,
    LAYOUT_FILE,
    PERSONA_STATE_FILE,
];
