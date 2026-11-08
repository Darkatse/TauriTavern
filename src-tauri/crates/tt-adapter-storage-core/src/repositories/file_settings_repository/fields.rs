use super::lazy_object::LazyObject;

pub(super) use tt_contracts::settings::{
    APPEARANCE_FILE, CORE_FILE, LAYOUT_FILE, PERSONA_STATE_FILE, PRESETS_FILE,
};
pub(super) const NATIVE_FILE: &str = "tauritavern-settings.json";
pub(super) const DYNAMIC_THEME_FILE: &str = "settings/dynamic-theme.json";

type FieldGroup = (&'static [&'static str], &'static [&'static str]);

// Keep the appearance snapshot aligned with getThemeObject in power-user.js.
const APPEARANCE_FIELDS: &[FieldGroup] = &[
    (&[], &["background"]),
    (
        &["power_user"],
        &[
            "theme",
            "theme_fallback",
            "theme_bindings",
            "blur_strength",
            "main_text_color",
            "italics_text_color",
            "underline_text_color",
            "quote_text_color",
            "blur_tint_color",
            "chat_tint_color",
            "user_mes_blur_tint_color",
            "bot_mes_blur_tint_color",
            "shadow_color",
            "shadow_width",
            "border_color",
            "font_scale",
            "fast_ui_mode",
            "waifuMode",
            "avatar_style",
            "chat_display",
            "toastr_position",
            "noShadows",
            "chat_width",
            "timer_enabled",
            "timestamps_enabled",
            "timestamp_model_icon",
            "mesIDDisplay_enabled",
            "hideChatAvatars_enabled",
            "message_token_count_enabled",
            "message_ttft_enabled",
            "message_token_rate_enabled",
            "message_cache_enabled",
            "expand_message_actions",
            "enableZenSliders",
            "enableLabMode",
            "hotswap_enabled",
            "custom_css",
            "bogus_folders",
            "zoomed_avatar_magnification",
            "reduced_motion",
            "compact_input_area",
            "show_swipe_num_all_messages",
            "click_to_edit",
            "media_display",
        ],
    ),
];

const PRESET_FIELDS: &[FieldGroup] = &[
    // These are the active preset snapshots, not the named preset libraries.
    // Keeping only their names would pair local names with another device's parameters.
    (
        &[],
        &[
            "main_api",
            "selected_proxy",
            "amount_gen",
            "max_context",
            "oai_settings",
            "nai_settings",
            "kai_settings",
            "textgenerationwebui_settings",
            "horde_settings",
            "preset_settings",
            "preset_settings_novel",
        ],
    ),
    (
        &["power_user"],
        &["instruct", "context", "sysprompt", "reasoning"],
    ),
    (
        &["extension_settings", "connectionManager"],
        &["selectedProfile", "selectedItem"],
    ),
];

const PERSONA_STATE_FIELDS: &[FieldGroup] = &[
    (&[], &["username", "user_avatar"]),
    (
        &["power_user"],
        &[
            "default_persona",
            "persona_description",
            "persona_description_position",
            "persona_description_depth",
            "persona_description_role",
            "persona_description_lorebook",
            "persona_show_notifications",
            "persona_auto_lock",
            "persona_allow_multi_connections",
        ],
    ),
];

const LAYOUT_FIELDS: &[FieldGroup] = &[
    (
        &[],
        &[
            "firstRun",
            "currentVersion",
            "active_character",
            "active_group",
            "selected_button",
        ],
    ),
    (
        &["power_user"],
        &[
            "movingUI",
            "movingUIState",
            "movingUIPreset",
            "mobile_immersive_fullscreen",
            "charListGrid",
            "sort_field",
            "sort_order",
            "sort_rule",
            "persona_sort_order",
            "show_tag_filters",
            "show_tag_filters_group_candidates",
            "show_tag_filters_group_members",
            "aux_field",
        ],
    ),
    // AccountStorage also contains extension data; only known UI state belongs here.
    (
        &["accountStorage"],
        &[
            "Characters_PerPage",
            "Personas_PerPage",
            "Personas_GridView",
            "GroupMembers_PerPage",
            "GroupCandidates_PerPage",
            "FeatherlessModels_PerPage",
            "WI_PerPage",
            "WINavLockOn",
            "WINavOpened",
            "LNavLockOn",
            "LNavOpened",
            "NavLockOn",
            "NavOpened",
            "SelectedNavTab",
            "characterSearchFormVisible",
            "world_info_sort_order",
            "DataBank_sortField",
            "DataBank_sortOrder",
        ],
    ),
];

/// Section files in revision order; the core goes last, after the sections split from it.
pub(super) const SECTION_FILES: [&str; 5] = [
    APPEARANCE_FILE,
    PRESETS_FILE,
    LAYOUT_FILE,
    PERSONA_STATE_FILE,
    CORE_FILE,
];

pub(super) struct UserSettingsSections {
    pub(super) core: LazyObject,
    pub(super) appearance: LazyObject,
    pub(super) presets: LazyObject,
    pub(super) layout: LazyObject,
    pub(super) persona_state: LazyObject,
}

impl UserSettingsSections {
    pub(super) fn split(mut settings: LazyObject) -> Self {
        let mut appearance = LazyObject::default();
        let mut presets = LazyObject::default();
        let mut layout = LazyObject::default();
        let mut persona_state = LazyObject::default();
        move_fields(&mut settings, &mut appearance, APPEARANCE_FIELDS);
        move_fields(&mut settings, &mut presets, PRESET_FIELDS);
        move_fields(&mut settings, &mut layout, LAYOUT_FIELDS);
        move_fields(&mut settings, &mut persona_state, PERSONA_STATE_FIELDS);
        Self {
            core: settings,
            appearance,
            presets,
            layout,
            persona_state,
        }
    }

    pub(super) fn into_settings(mut self) -> LazyObject {
        // A selected section cannot overwrite fields owned by another section.
        move_fields(&mut self.appearance, &mut self.core, APPEARANCE_FIELDS);
        move_fields(&mut self.presets, &mut self.core, PRESET_FIELDS);
        move_fields(&mut self.layout, &mut self.core, LAYOUT_FIELDS);
        move_fields(
            &mut self.persona_state,
            &mut self.core,
            PERSONA_STATE_FIELDS,
        );
        self.core
    }

    /// The sections in [`SECTION_FILES`] order.
    pub(super) fn into_files(self) -> [LazyObject; 5] {
        [
            self.appearance,
            self.presets,
            self.layout,
            self.persona_state,
            self.core,
        ]
    }

    pub(super) fn from_files(
        [appearance, presets, layout, persona_state, core]: [LazyObject; 5],
    ) -> Self {
        Self {
            core,
            appearance,
            presets,
            layout,
            persona_state,
        }
    }
}

fn move_fields(source: &mut LazyObject, target: &mut LazyObject, groups: &[FieldGroup]) {
    for (parents, fields) in groups {
        let Some(source_object) = source.object_mut(parents) else {
            continue;
        };
        for field in *fields {
            let Some(member) = source_object.remove(field) else {
                continue;
            };
            target.object_entry(parents).insert(field, member);
        }
    }
}
