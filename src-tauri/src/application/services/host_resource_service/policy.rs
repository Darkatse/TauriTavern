use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub(crate) struct HostResourceRuntimePolicy {
    avatar_persona_original_images_enabled: AtomicBool,
}

impl HostResourceRuntimePolicy {
    pub(crate) fn new(avatar_persona_original_images_enabled: bool) -> Self {
        Self {
            avatar_persona_original_images_enabled: AtomicBool::new(
                avatar_persona_original_images_enabled,
            ),
        }
    }

    pub(crate) fn avatar_persona_original_images_enabled(&self) -> bool {
        self.avatar_persona_original_images_enabled
            .load(Ordering::Relaxed)
    }

    pub(crate) fn set_avatar_persona_original_images_enabled(&self, enabled: bool) {
        self.avatar_persona_original_images_enabled
            .store(enabled, Ordering::Relaxed);
    }
}
