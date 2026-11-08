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
    /// Present while the host draws the page's top-level drawers in a system bar region.
    pub drawer_bar: Option<DrawerBar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerBar {
    pub edge: DrawerBarEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DrawerBarEdge {
    Left,
    Right,
    Top,
}

/// A top-level drawer as the page presents it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerBarItem {
    pub id: String,
    pub title: String,
    pub open: bool,
    pub mark: Option<DrawerBarMark>,
}

/// A state the page shows on the drawer's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DrawerBarMark {
    /// The drawer needs the user, such as when the API is disconnected.
    Alert,
    /// Work started from the drawer is running.
    Working,
    /// The drawer holds a result the user has not seen.
    Unread,
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
