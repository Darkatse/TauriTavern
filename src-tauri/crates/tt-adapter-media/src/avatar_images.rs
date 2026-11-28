use std::io::Cursor;

use async_trait::async_trait;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use tt_adapter_storage_core::png_metadata::PNG_SIGNATURE;
use tt_domain::errors::DomainError;
use tt_ports::avatar_images::{AvatarImageProcessor, ImageCrop};

const AVATAR_WIDTH: u32 = 400;
const AVATAR_HEIGHT: u32 = 600;

pub struct MediaAvatarImageProcessor;

#[async_trait]
impl AvatarImageProcessor for MediaAvatarImageProcessor {
    async fn process(
        &self,
        image: Vec<u8>,
        crop: Option<ImageCrop>,
    ) -> Result<Vec<u8>, DomainError> {
        tokio::task::spawn_blocking(move || avatar_png(image, crop.as_ref()))
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to join avatar image processor: {error}"
                ))
            })?
    }
}

pub(crate) fn avatar_png(image: Vec<u8>, crop: Option<&ImageCrop>) -> Result<Vec<u8>, DomainError> {
    let decoded = image::load_from_memory(&image)
        .map_err(|error| DomainError::InvalidData(format!("Failed to load image: {error}")))?;
    match crop {
        Some(crop) => encode_png(&apply_crop(&decoded, crop)?),
        None if image.starts_with(&PNG_SIGNATURE) => Ok(image),
        None => encode_png(&decoded),
    }
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, DomainError> {
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| DomainError::InternalError(format!("Failed to encode avatar: {error}")))?;
    Ok(png.into_inner())
}

fn apply_crop(image: &DynamicImage, crop: &ImageCrop) -> Result<DynamicImage, DomainError> {
    let (Some((x, width)), Some((y, height))) = (
        inside(crop.x, crop.width, image.width()),
        inside(crop.y, crop.height, image.height()),
    ) else {
        return Err(DomainError::InvalidData(format!(
            "Avatar crop selects no pixels: {crop:?}"
        )));
    };
    let cropped = image.crop_imm(x, y, width, height);
    Ok(if crop.want_resize {
        cropped.resize_to_fill(AVATAR_WIDTH, AVATAR_HEIGHT, FilterType::Lanczos3)
    } else {
        cropped
    })
}

/// The part of `[start, start + length)` within `[0, limit)`, as start and length.
fn inside(start: i32, length: i32, limit: u32) -> Option<(u32, u32)> {
    let end = (i64::from(start) + i64::from(length)).min(i64::from(limit));
    let start = i64::from(start).max(0);
    (start < end).then(|| (start as u32, (end - start) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgba, RgbaImage};
    use tt_adapter_storage_core::png_metadata::{replace_text_chunks, write_chunk};

    fn size(png: &[u8]) -> (u32, u32) {
        image::load_from_memory(png).unwrap().dimensions()
    }

    #[test]
    fn avatars_keep_their_size_unless_a_crop_asks_for_the_standard_one() {
        let pixels = RgbaImage::from_pixel(800, 400, Rgba([0, 0, 255, 255]));
        let mut png = Cursor::new(Vec::new());
        pixels.write_to(&mut png, ImageFormat::Png).unwrap();
        let mut private_chunk = Vec::new();
        write_chunk(&mut private_chunk, *b"prIv", b"kept");
        let png = replace_text_chunks(&png.into_inner(), &[], &private_chunk).unwrap();

        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(pixels)
            .to_rgb8()
            .write_to(&mut jpeg, ImageFormat::Jpeg)
            .unwrap();
        let jpeg = jpeg.into_inner();

        let crop = |x, y, width, height, want_resize| ImageCrop {
            x,
            y,
            width,
            height,
            want_resize,
        };

        assert_eq!(avatar_png(png.clone(), None).unwrap(), png);
        let converted = avatar_png(jpeg, None).unwrap();
        assert!(converted.starts_with(&PNG_SIGNATURE));
        assert_eq!(size(&converted), (800, 400));
        assert!(avatar_png(b"\x89PNG\r\n\x1a\ncorrupt".to_vec(), None).is_err());
        assert_eq!(
            size(&avatar_png(png.clone(), Some(&crop(100, 0, 200, 300, true))).unwrap()),
            (AVATAR_WIDTH, AVATAR_HEIGHT)
        );
        assert_eq!(
            size(&avatar_png(png.clone(), Some(&crop(601, 101, 200, 300, false))).unwrap()),
            (199, 299)
        );
        assert_eq!(
            size(&avatar_png(png.clone(), Some(&crop(-10, -10, 110, 210, false))).unwrap()),
            (100, 200)
        );
        assert!(avatar_png(png, Some(&crop(800, 0, 10, 10, true))).is_err());
    }
}
