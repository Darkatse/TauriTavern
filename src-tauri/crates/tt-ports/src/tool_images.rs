/// Formats accepted by every supported image provider.
#[derive(Debug, Clone, Copy)]
pub enum ToolImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ToolImageFormat {
    pub fn properties(self) -> (&'static str, &'static str) {
        match self {
            Self::Png => ("image/png", "png"),
            Self::Jpeg => ("image/jpeg", "jpg"),
            Self::Webp => ("image/webp", "webp"),
        }
    }
}

/// Longest edge of a stored tool image, in pixels.
pub const MAX_TOOL_IMAGE_EDGE: u32 = 1280;
pub const MAX_TOOL_IMAGE_BYTES: usize = 1_000_000;

pub trait ToolImageNormalizer: Send + Sync {
    fn normalize(&self, data: &[u8]) -> Result<StoredToolImage, ToolImageRejection>;
}

pub struct StoredToolImage {
    pub format: ToolImageFormat,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum ToolImageRejection {
    Undecodable,
    EncodingFailed(String),
    TooLarge { bytes: usize },
}
