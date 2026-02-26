use crate::domain::errors::DomainError;
use crate::domain::repositories::character_repository::ImageCrop;
use crate::infrastructure::logging::logger;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::ImageFormat;
use png::{BitDepth, ColorType, Transformations, text_metadata::TEXtChunk};
use std::io::Cursor;

/// PNG text keys used for character data.
const CHUNK_NAME_V2: &str = "chara";
const CHUNK_NAME_V3: &str = "ccv3";

/// Logical text entry parsed from PNG metadata (tEXt/zTXt/iTXt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub keyword: String,
    pub text: String,
}

#[derive(Debug)]
struct DecodedPngFrame {
    width: u32,
    height: u32,
    color_type: ColorType,
    bit_depth: BitDepth,
    palette: Option<Vec<u8>>,
    trns: Option<Vec<u8>>,
    image_data: Vec<u8>,
}

/// Reads all text metadata chunks from a PNG image.
///
/// This includes `tEXt`, `zTXt`, and `iTXt` chunks.
pub fn read_text_chunks_from_png(image_data: &[u8]) -> Result<Vec<TextChunk>, DomainError> {
    let mut decoder = png::Decoder::new(Cursor::new(image_data));
    decoder.set_transformations(Transformations::IDENTITY);

    let mut reader = decoder
        .read_info()
        .map_err(|e| DomainError::InvalidData(format!("Failed to read PNG header: {}", e)))?;

    // Parse the entire stream so metadata placed after IDAT (common for card files) is included.
    reader
        .finish()
        .map_err(|e| DomainError::InvalidData(format!("Failed to parse PNG metadata: {}", e)))?;

    let info = reader.info();
    let mut chunks = Vec::new();

    chunks.extend(info.uncompressed_latin1_text.iter().map(|chunk| TextChunk {
        keyword: chunk.keyword.clone(),
        text: chunk.text.clone(),
    }));

    for chunk in &info.compressed_latin1_text {
        let text = chunk.get_text().map_err(|e| {
            DomainError::InvalidData(format!("Failed to decode zTXt metadata: {}", e))
        })?;

        chunks.push(TextChunk {
            keyword: chunk.keyword.clone(),
            text,
        });
    }

    for chunk in &info.utf8_text {
        let text = chunk.get_text().map_err(|e| {
            DomainError::InvalidData(format!("Failed to decode iTXt metadata: {}", e))
        })?;

        chunks.push(TextChunk {
            keyword: chunk.keyword.clone(),
            text,
        });
    }

    Ok(chunks)
}

/// Reads character data from PNG metadata.
///
/// It prefers V3 (`ccv3`) and falls back to V2 (`chara`).
pub fn read_character_data_from_png(image_data: &[u8]) -> Result<String, DomainError> {
    tracing::debug!("Reading character data from PNG");

    let text_chunks = read_text_chunks_from_png(image_data)?;

    if text_chunks.is_empty() {
        return Err(DomainError::InvalidData(
            "PNG metadata does not contain any text chunks".to_string(),
        ));
    }

    if let Some(chunk) = find_first_text_chunk(&text_chunks, CHUNK_NAME_V3) {
        return decode_base64(chunk);
    }

    if let Some(chunk) = find_first_text_chunk(&text_chunks, CHUNK_NAME_V2) {
        return decode_base64(chunk);
    }

    Err(DomainError::InvalidData(
        "PNG metadata does not contain character data".to_string(),
    ))
}

/// Writes character data to PNG metadata.
///
/// The image pixel data is kept and the metadata is rewritten using the `png` crate.
/// Character chunks are always emitted as `tEXt`: `chara` (V2) and, when possible, `ccv3` (V3).
pub fn write_character_data_to_png(
    image_data: &[u8],
    character_data: &str,
) -> Result<Vec<u8>, DomainError> {
    tracing::debug!("Writing character data to PNG");

    let decoded = decode_png_frame(image_data)?;

    let mut character_chunks = Vec::with_capacity(2);
    character_chunks.push(TextChunk {
        keyword: CHUNK_NAME_V2.to_string(),
        text: encode_base64(character_data),
    });

    if let Some(v3_payload) = build_v3_payload(character_data)? {
        character_chunks.push(TextChunk {
            keyword: CHUNK_NAME_V3.to_string(),
            text: v3_payload,
        });
    }

    encode_png_with_text_chunks(&decoded, &character_chunks)
}

/// Process an image for use as a character avatar.
pub async fn process_avatar_image(
    image_data: &[u8],
    crop: Option<ImageCrop>,
) -> Result<Vec<u8>, DomainError> {
    tracing::debug!("Processing avatar image");

    // Load the image
    let mut img = image::load_from_memory(image_data)
        .map_err(|e| DomainError::InvalidData(format!("Failed to load image: {}", e)))?;

    // Apply crop if defined
    if let Some(crop_params) = crop {
        if crop_params.x >= 0
            && crop_params.y >= 0
            && crop_params.width > 0
            && crop_params.height > 0
            && (crop_params.x as u32 + crop_params.width as u32) <= img.width()
            && (crop_params.y as u32 + crop_params.height as u32) <= img.height()
        {
            img = img.crop(
                crop_params.x as u32,
                crop_params.y as u32,
                crop_params.width as u32,
                crop_params.height as u32,
            );

            // Apply standard resize if requested
            if crop_params.want_resize {
                // Use the standard avatar dimensions from SillyTavern
                const AVATAR_WIDTH: u32 = 400;
                const AVATAR_HEIGHT: u32 = 600;

                img = img.resize_to_fill(
                    AVATAR_WIDTH,
                    AVATAR_HEIGHT,
                    image::imageops::FilterType::Lanczos3,
                );
            }
        } else {
            logger::warn("Invalid crop parameters, ignoring crop");
        }
    }

    // Convert to PNG
    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);

    img.write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| DomainError::InternalError(format!("Failed to write PNG image: {}", e)))?;

    Ok(output)
}

fn find_first_text_chunk<'a>(chunks: &'a [TextChunk], keyword: &str) -> Option<&'a str> {
    chunks
        .iter()
        .find(|chunk| chunk.keyword.eq_ignore_ascii_case(keyword))
        .map(|chunk| chunk.text.as_str())
}

fn encode_base64(data: &str) -> String {
    BASE64.encode(data.as_bytes())
}

fn decode_base64(data: &str) -> Result<String, DomainError> {
    let bytes = BASE64
        .decode(data.trim())
        .map_err(|e| DomainError::InvalidData(format!("Failed to decode base64: {}", e)))?;

    String::from_utf8(bytes)
        .map_err(|e| DomainError::InvalidData(format!("Failed to convert from UTF-8: {}", e)))
}

fn build_v3_payload(character_data: &str) -> Result<Option<String>, DomainError> {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(character_data) else {
        return Ok(None);
    };

    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };

    object.insert(
        "spec".to_string(),
        serde_json::Value::String("chara_card_v3".to_string()),
    );
    object.insert(
        "spec_version".to_string(),
        serde_json::Value::String("3.0".to_string()),
    );

    let serialized = serde_json::to_string(&value).map_err(|e| {
        DomainError::InvalidData(format!("Failed to serialize V3 card data: {}", e))
    })?;

    Ok(Some(encode_base64(&serialized)))
}

fn decode_png_frame(image_data: &[u8]) -> Result<DecodedPngFrame, DomainError> {
    let mut decoder = png::Decoder::new(Cursor::new(image_data));
    decoder.set_transformations(Transformations::IDENTITY);

    let mut reader = decoder
        .read_info()
        .map_err(|e| DomainError::InvalidData(format!("Failed to read PNG header: {}", e)))?;

    let mut buffer = vec![0; reader.output_buffer_size()];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|e| DomainError::InvalidData(format!("Failed to decode PNG image data: {}", e)))?;

    buffer.truncate(output.buffer_size());

    let info = reader.info();

    if output.color_type == ColorType::Indexed && info.palette.is_none() {
        return Err(DomainError::InvalidData(
            "Indexed PNG is missing palette data".to_string(),
        ));
    }

    Ok(DecodedPngFrame {
        width: output.width,
        height: output.height,
        color_type: output.color_type,
        bit_depth: output.bit_depth,
        palette: info.palette.as_ref().map(|p| p.to_vec()),
        trns: info.trns.as_ref().map(|t| t.to_vec()),
        image_data: buffer,
    })
}

fn encode_png_with_text_chunks(
    decoded: &DecodedPngFrame,
    text_chunks: &[TextChunk],
) -> Result<Vec<u8>, DomainError> {
    let mut output = Vec::new();

    {
        let mut encoder = png::Encoder::new(&mut output, decoded.width, decoded.height);
        encoder.set_color(decoded.color_type);
        encoder.set_depth(decoded.bit_depth);

        if let Some(palette) = &decoded.palette {
            encoder.set_palette(palette.clone());
        }

        if let Some(trns) = &decoded.trns {
            encoder.set_trns(trns.clone());
        }

        let mut writer = encoder.write_header().map_err(|e| {
            DomainError::InternalError(format!("Failed to write PNG header: {}", e))
        })?;

        writer.write_image_data(&decoded.image_data).map_err(|e| {
            DomainError::InternalError(format!("Failed to write PNG image data: {}", e))
        })?;

        for chunk in text_chunks {
            let text_chunk = TEXtChunk::new(chunk.keyword.clone(), chunk.text.clone());
            writer.write_text_chunk(&text_chunk).map_err(|e| {
                DomainError::InternalError(format!("Failed to write PNG text metadata: {}", e))
            })?;
        }

        writer.finish().map_err(|e| {
            DomainError::InternalError(format!("Failed to finalize PNG file: {}", e))
        })?;
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{
        TextChunk, decode_base64, decode_png_frame, encode_base64, read_character_data_from_png,
        read_text_chunks_from_png, write_character_data_to_png,
    };
    use image::{DynamicImage, ImageFormat, RgbaImage};
    use png::text_metadata::{ITXtChunk, TEXtChunk};
    use serde_json::Value;
    use std::io::Cursor;

    fn build_minimal_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::new(1, 1));
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);
        image
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("should build png");
        output
    }

    fn build_png_with_text_chunks(text_chunks: &[TextChunk]) -> Vec<u8> {
        let base_png = build_minimal_png();
        let decoded = decode_png_frame(&base_png).expect("decode base png");

        let mut output = Vec::new();
        let mut encoder = png::Encoder::new(&mut output, decoded.width, decoded.height);
        encoder.set_color(decoded.color_type);
        encoder.set_depth(decoded.bit_depth);

        if let Some(palette) = decoded.palette {
            encoder.set_palette(palette);
        }

        if let Some(trns) = decoded.trns {
            encoder.set_trns(trns);
        }

        let mut writer = encoder.write_header().expect("write header");
        writer
            .write_image_data(&decoded.image_data)
            .expect("write image data");

        for chunk in text_chunks {
            let text_chunk = TEXtChunk::new(chunk.keyword.clone(), chunk.text.clone());
            writer
                .write_text_chunk(&text_chunk)
                .expect("write text metadata");
        }

        writer.finish().expect("finish png");
        output
    }

    #[test]
    fn write_replaces_existing_character_metadata_chunks() {
        let base_png = build_minimal_png();
        let first_json =
            r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina","chat":"old-chat"}"#;
        let second_json =
            r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina","chat":"new-chat"}"#;

        let first_write =
            write_character_data_to_png(&base_png, first_json).expect("first write succeeds");
        let second_write =
            write_character_data_to_png(&first_write, second_json).expect("second write succeeds");

        let text_chunks = read_text_chunks_from_png(&second_write).expect("read text metadata");
        let character_chunks_count = text_chunks
            .iter()
            .filter(|chunk| {
                chunk.keyword.eq_ignore_ascii_case("chara")
                    || chunk.keyword.eq_ignore_ascii_case("ccv3")
            })
            .count();

        // Exactly two metadata chunks should remain: one `chara`, one `ccv3`.
        assert_eq!(character_chunks_count, 2);

        let decoded = read_character_data_from_png(&second_write).expect("read should succeed");
        let parsed: Value = serde_json::from_str(&decoded).expect("valid json");
        assert_eq!(parsed.get("chat").and_then(Value::as_str), Some("new-chat"));
    }

    #[test]
    fn read_prefers_first_duplicate_metadata_chunk() {
        let old_json =
            r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina","chat":"old-chat"}"#;
        let new_json =
            r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina","chat":"new-chat"}"#;

        let old_payload = encode_base64(old_json);
        let new_payload = encode_base64(new_json);

        let png_with_duplicates = build_png_with_text_chunks(&[
            TextChunk {
                keyword: "chara".to_string(),
                text: old_payload,
            },
            TextChunk {
                keyword: "chara".to_string(),
                text: new_payload,
            },
        ]);

        let decoded =
            read_character_data_from_png(&png_with_duplicates).expect("read should succeed");
        let parsed: Value = serde_json::from_str(&decoded).expect("valid json");

        assert_eq!(parsed.get("chat").and_then(Value::as_str), Some("old-chat"));

        // Sanity check: base64 helper roundtrip.
        assert_eq!(
            decode_base64(&encode_base64(new_json)).expect("decode"),
            new_json
        );
    }

    #[test]
    fn read_supports_itxt_metadata() {
        let base_png = build_minimal_png();
        let decoded = decode_png_frame(&base_png).expect("decode base png");
        let json = r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina"}"#;
        let encoded = encode_base64(json);

        let mut output = Vec::new();
        let mut encoder = png::Encoder::new(&mut output, decoded.width, decoded.height);
        encoder.set_color(decoded.color_type);
        encoder.set_depth(decoded.bit_depth);

        let mut writer = encoder.write_header().expect("write header");
        writer
            .write_image_data(&decoded.image_data)
            .expect("write image data");

        let itxt = ITXtChunk::new("ccv3".to_string(), encoded);
        writer.write_text_chunk(&itxt).expect("write iTXt chunk");
        writer.finish().expect("finish png");

        let parsed = read_character_data_from_png(&output).expect("read should succeed");
        let parsed_json: Value = serde_json::from_str(&parsed).expect("valid json");

        assert_eq!(
            parsed_json.get("name").and_then(Value::as_str),
            Some("Seraphina")
        );
    }
}
