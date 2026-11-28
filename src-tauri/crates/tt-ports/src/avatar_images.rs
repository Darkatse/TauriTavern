use async_trait::async_trait;
use tt_domain::errors::DomainError;

/// A region chosen in the avatar crop dialog, in source image pixels. The part
/// outside the image is dropped; a region with no pixels inside is rejected.
#[derive(Debug, Clone)]
pub struct ImageCrop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The cropped region fills the standard avatar size.
    pub want_resize: bool,
}

/// Turns an uploaded image into the PNG stored as a character or Persona avatar.
#[async_trait]
pub trait AvatarImageProcessor: Send + Sync {
    /// Without a crop a valid PNG is kept verbatim and other formats keep their size.
    async fn process(
        &self,
        image: Vec<u8>,
        crop: Option<ImageCrop>,
    ) -> Result<Vec<u8>, DomainError>;
}
