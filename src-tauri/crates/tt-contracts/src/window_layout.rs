//! Native window coordinates are physical pixels; CSS coordinates divide by scale.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInsets {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSnapshot {
    pub revision: u64,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub insets: WindowInsets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowBackdropRequest {
    pub window_revision: u64,
    pub color: [u8; 3],
    /// Omitted for a color-only update; a null resource_path explicitly clears wallpaper.
    pub wallpaper: Option<WindowWallpaper>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowWallpaper {
    /// A browser resource path, never a filesystem path or a remote URL.
    pub resource_path: Option<String>,
    pub size: String,
    pub position: String,
}
