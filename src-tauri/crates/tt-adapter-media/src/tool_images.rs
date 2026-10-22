use image::{ImageFormat, Rgb, RgbImage, codecs::jpeg::JpegEncoder, imageops::FilterType};
use tt_ports::tool_images::{
    MAX_TOOL_IMAGE_BYTES, MAX_TOOL_IMAGE_EDGE, StoredToolImage, TOOL_IMAGE_TYPES,
    ToolImageNormalizer, ToolImageRejection,
};

pub struct MediaToolImageNormalizer;

impl ToolImageNormalizer for MediaToolImageNormalizer {
    fn normalize(&self, data: &[u8]) -> Result<StoredToolImage, ToolImageRejection> {
        let format = image::guess_format(data).map_err(|_| ToolImageRejection::Undecodable)?;
        let image = image::load_from_memory_with_format(data, format)
            .map_err(|_| ToolImageRejection::Undecodable)?;
        let mime_type = format.to_mime_type();
        if TOOL_IMAGE_TYPES.contains(&mime_type)
            && image.width().max(image.height()) <= MAX_TOOL_IMAGE_EDGE
            && data.len() <= MAX_TOOL_IMAGE_BYTES
        {
            return Ok(StoredToolImage {
                mime_type,
                data: data.to_vec(),
            });
        }
        let image = if image.width().max(image.height()) > MAX_TOOL_IMAGE_EDGE {
            image.resize(
                MAX_TOOL_IMAGE_EDGE,
                MAX_TOOL_IMAGE_EDGE,
                FilterType::Triangle,
            )
        } else {
            image
        }
        .to_rgba8();
        let mut opaque = RgbImage::new(image.width(), image.height());
        for (target, source) in opaque.pixels_mut().zip(image.pixels()) {
            let alpha = u32::from(source[3]);
            *target = Rgb(std::array::from_fn(|channel| {
                ((u32::from(source[channel]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
            }));
        }
        let mut data = Vec::new();
        JpegEncoder::new_with_quality(&mut data, 85)
            .encode_image(&opaque)
            .map_err(|_| ToolImageRejection::Undecodable)?;
        if data.len() > MAX_TOOL_IMAGE_BYTES {
            return Err(ToolImageRejection::TooLarge { bytes: data.len() });
        }
        Ok(StoredToolImage {
            mime_type: ImageFormat::Jpeg.to_mime_type(),
            data,
        })
    }
}
