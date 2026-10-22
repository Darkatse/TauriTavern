/// Formats accepted by every supported image provider.
#[derive(Debug, Clone, Copy)]
pub enum ToolImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ToolImageFormat {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

/// Longest edge of a stored tool image, in pixels. `CAPTURE_MAX_EDGE` in
/// `packages/app-use/src/capture.ts` must not exceed it, or captures are resampled and re-encoded twice.
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
    TooLarge { bytes: usize },
}
