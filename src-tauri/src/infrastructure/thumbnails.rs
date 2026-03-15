use crate::infrastructure::persistence::thumbnail_cache::{ThumbnailConfig, ThumbnailResizeMode};

pub const AVATAR_THUMBNAIL_WIDTH: u32 = 96;
pub const AVATAR_THUMBNAIL_HEIGHT: u32 = 144;
pub const AVATAR_THUMBNAIL_QUALITY: u8 = 90;

pub const BACKGROUND_THUMBNAIL_WIDTH: u32 = 160;
pub const BACKGROUND_THUMBNAIL_HEIGHT: u32 = 90;
pub const BACKGROUND_THUMBNAIL_QUALITY: u8 = 90;

pub fn avatar_thumbnail_config() -> ThumbnailConfig {
    ThumbnailConfig {
        width: AVATAR_THUMBNAIL_WIDTH,
        height: AVATAR_THUMBNAIL_HEIGHT,
        quality: AVATAR_THUMBNAIL_QUALITY,
        resize_mode: ThumbnailResizeMode::Cover,
    }
}

pub fn background_thumbnail_config() -> ThumbnailConfig {
    ThumbnailConfig {
        width: BACKGROUND_THUMBNAIL_WIDTH,
        height: BACKGROUND_THUMBNAIL_HEIGHT,
        quality: BACKGROUND_THUMBNAIL_QUALITY,
        resize_mode: ThumbnailResizeMode::PreserveArea,
    }
}
