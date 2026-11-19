use serde::{Deserialize, Serialize};
use tt_ports::avatar_images::ImageCrop;

/// Image crop DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCropDto {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub want_resize: bool,
}

impl From<ImageCropDto> for ImageCrop {
    fn from(dto: ImageCropDto) -> Self {
        Self {
            x: dto.x,
            y: dto.y,
            width: dto.width,
            height: dto.height,
            want_resize: dto.want_resize,
        }
    }
}
