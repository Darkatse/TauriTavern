/// Formats accepted by every supported image provider.
pub const TOOL_IMAGE_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];
/// Longest edge of a stored tool image, in pixels.
pub const MAX_TOOL_IMAGE_EDGE: u32 = 1280;
pub const MAX_TOOL_IMAGE_BYTES: usize = 1_000_000;

pub trait ToolImageNormalizer: Send + Sync {
    fn normalize(&self, data: &[u8]) -> Result<StoredToolImage, ToolImageRejection>;
}

pub struct StoredToolImage {
    pub mime_type: &'static str,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum ToolImageRejection {
    Undecodable,
    TooLarge { bytes: usize },
}
