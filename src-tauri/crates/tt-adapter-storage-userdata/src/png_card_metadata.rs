use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::ImageFormat;
use std::io::Cursor;
use std::path::Path;
#[cfg(test)]
use tt_adapter_storage_core::png_metadata::write_chunk;
#[cfg(test)]
use tt_adapter_storage_core::png_metadata::{
    CHUNK_TYPE_IEND, CHUNK_TYPE_ITXT, CHUNK_TYPE_ZTXT, PNG_SIGNATURE, read_next_png_chunk,
};
pub use tt_adapter_storage_core::png_metadata::{TextChunk, read_text_chunks_from_png};
use tt_adapter_storage_core::png_metadata::{
    read_preferred_text_chunk, replace_text_chunks, write_text_chunk,
};
use tt_domain::errors::DomainError;
use tt_ports::repositories::character_repository::ImageCrop;

/// PNG text keys used for character data.
const CHUNK_NAME_V2: &str = "chara";
const CHUNK_NAME_V3: &str = "ccv3";

/// V3 takes precedence; once found, unrelated metadata cannot invalidate the card.
pub fn read_character_data_from_png(image_data: &[u8]) -> Result<String, DomainError> {
    decode_character_chunk(read_preferred_text_chunk(
        Cursor::new(image_data),
        &[CHUNK_NAME_V3, CHUNK_NAME_V2],
    )?)
}

/// Read metadata without loading or decoding image pixels.
pub async fn read_character_data_from_png_file(path: &Path) -> Result<String, DomainError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path).map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to open character PNG {}: {error}",
                path.display()
            ))
        })?;
        decode_character_chunk(read_preferred_text_chunk(
            file,
            &[CHUNK_NAME_V3, CHUNK_NAME_V2],
        )?)
    })
    .await
    .map_err(|error| DomainError::InternalError(error.to_string()))?
}

fn decode_character_chunk(chunk: Option<TextChunk>) -> Result<String, DomainError> {
    let chunk = chunk.ok_or_else(|| {
        DomainError::InvalidData("PNG metadata does not contain character data".into())
    })?;
    decode_base64(&chunk.text)
}

/// Writes character data to PNG metadata.
///
/// Performs a chunk-level rewrite: preserves all existing chunks except the character metadata
/// chunks (`tEXt` `chara` / `ccv3`), and injects new metadata before `IEND`.
///
/// Character chunks are emitted as `tEXt`: `chara` (V2) and, when possible, `ccv3` (V3),
/// matching upstream SillyTavern behavior.
pub fn write_character_data_to_png(
    image_data: &[u8],
    character_data: &str,
) -> Result<Vec<u8>, DomainError> {
    tracing::debug!("Writing character data to PNG");

    let mut chunks = Vec::new();
    write_text_chunk(&mut chunks, CHUNK_NAME_V2, &encode_base64(character_data));
    if let Some(payload) = build_v3_payload(character_data)? {
        write_text_chunk(&mut chunks, CHUNK_NAME_V3, &payload);
    }
    replace_text_chunks(image_data, &[CHUNK_NAME_V2, CHUNK_NAME_V3], &chunks)
}

/// Process an image for use as a character avatar.
pub async fn process_avatar_image(
    image_data: Vec<u8>,
    crop: Option<ImageCrop>,
) -> Result<Vec<u8>, DomainError> {
    tracing::debug!("Processing avatar image");

    tokio::task::spawn_blocking(move || process_avatar_image_sync(&image_data, crop))
        .await
        .map_err(|error| {
            DomainError::InternalError(format!("Failed to join avatar image processor: {}", error))
        })?
}

fn process_avatar_image_sync(
    image_data: &[u8],
    crop: Option<ImageCrop>,
) -> Result<Vec<u8>, DomainError> {
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
            tracing::warn!("Invalid crop parameters, ignoring crop");
        }
    }

    // Convert to PNG
    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);

    img.write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| DomainError::InternalError(format!("Failed to write PNG image: {}", e)))?;

    Ok(output)
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

#[cfg(test)]
mod tests {
    use super::{
        CHUNK_TYPE_IEND, CHUNK_TYPE_ITXT, CHUNK_TYPE_ZTXT, PNG_SIGNATURE, decode_base64,
        encode_base64, read_character_data_from_png, read_character_data_from_png_file,
        read_next_png_chunk, read_text_chunks_from_png, write_character_data_to_png, write_chunk,
        write_text_chunk,
    };
    use flate2::{Compression, write::ZlibEncoder};
    use image::{DynamicImage, ImageFormat, RgbaImage};
    use rand::random;
    use serde_json::Value;
    use std::io::{Cursor, Write};
    use tokio::fs;

    fn build_minimal_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::new(1, 1));
        let mut output = Vec::new();
        let mut cursor = Cursor::new(&mut output);
        image
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("should build png");
        output
    }

    async fn read_streamed_temp(png: &[u8]) -> Result<String, tt_domain::errors::DomainError> {
        let path =
            std::env::temp_dir().join(format!("tauritavern-png-stream-{}.png", random::<u64>()));

        fs::write(&path, png).await.expect("write png");
        let result = read_character_data_from_png_file(&path).await;
        let _ = fs::remove_file(&path).await;
        result
    }

    fn inject_raw_chunks_before_iend(base_png: &[u8], raw_chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(&PNG_SIGNATURE);

        let mut offset = PNG_SIGNATURE.len();
        while let Some(chunk) = read_next_png_chunk(base_png, &mut offset).expect("read chunk") {
            if chunk.chunk_type == CHUNK_TYPE_IEND {
                for extra in raw_chunks {
                    output.extend_from_slice(extra);
                }
                output.extend_from_slice(chunk.raw);
                break;
            }

            output.extend_from_slice(chunk.raw);
        }

        output
    }

    fn build_ztxt_chunk(keyword: &str, text: &str) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(text.as_bytes())
            .expect("compress zTXt payload");
        let compressed = encoder.finish().expect("finish zTXt compression");

        let mut data = Vec::new();
        data.extend_from_slice(keyword.as_bytes());
        data.push(0);
        data.push(0);
        data.extend_from_slice(&compressed);

        let mut chunk = Vec::new();
        write_chunk(&mut chunk, CHUNK_TYPE_ZTXT, &data);
        chunk
    }

    fn build_itxt_chunk(keyword: &str, text: &str) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(keyword.as_bytes());
        data.push(0);
        data.push(0);
        data.push(0);
        data.push(0);
        data.push(0);
        data.extend_from_slice(text.as_bytes());

        let mut chunk = Vec::new();
        write_chunk(&mut chunk, CHUNK_TYPE_ITXT, &data);
        chunk
    }

    #[tokio::test]
    async fn streaming_read_supports_compressed_text_metadata() {
        let base_png = build_minimal_png();
        let json = r#"{"spec":"chara_card_v3","spec_version":"3.0","name":"Streamed"}"#;
        let encoded = encode_base64(json);

        for chunk in [
            build_ztxt_chunk("ccv3", &encoded),
            build_itxt_chunk("ccv3", &encoded),
        ] {
            let png = inject_raw_chunks_before_iend(&base_png, &[chunk]);
            let streamed = read_streamed_temp(&png).await.expect("stream metadata");
            assert_eq!(streamed, json);
        }
    }

    #[tokio::test]
    async fn valid_v3_does_not_decode_later_broken_v2_metadata() {
        let json = r#"{"spec":"chara_card_v3","spec_version":"3.0","name":"Keep"}"#;
        let mut preferred = Vec::new();
        write_text_chunk(&mut preferred, "ccv3", &encode_base64(json));
        let mut broken = Vec::new();
        write_chunk(&mut broken, CHUNK_TYPE_ZTXT, b"chara\0\0broken");
        let png = inject_raw_chunks_before_iend(&build_minimal_png(), &[preferred, broken]);
        assert_eq!(read_streamed_temp(&png).await.unwrap(), json);
    }

    #[tokio::test]
    async fn streaming_read_rejects_truncated_chunks() {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&10u32.to_be_bytes());
        png.extend_from_slice(b"IDAT");
        png.extend_from_slice(&[1, 2]);

        let error = read_streamed_temp(&png)
            .await
            .expect_err("truncated chunk should fail");
        assert!(error.to_string().contains("truncated PNG chunk"), "{error}");
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
    fn write_removes_existing_character_metadata_from_all_text_chunk_types() {
        let base_png = build_minimal_png();
        let old_json =
            r#"{"spec":"chara_card_v3","spec_version":"3.0","name":"Seraphina","chat":"old-chat"}"#;
        let new_json =
            r#"{"spec":"chara_card_v3","spec_version":"3.0","name":"Seraphina","chat":"new-chat"}"#;
        let old_payload = encode_base64(old_json);

        let png_with_old_metadata = inject_raw_chunks_before_iend(
            &base_png,
            &[
                build_ztxt_chunk("chara", &old_payload),
                build_itxt_chunk("ccv3", &old_payload),
            ],
        );
        let old_decoded =
            read_character_data_from_png(&png_with_old_metadata).expect("read old metadata");
        let old_parsed: Value = serde_json::from_str(&old_decoded).expect("valid old json");
        assert_eq!(
            old_parsed.get("chat").and_then(Value::as_str),
            Some("old-chat")
        );

        let rewritten = write_character_data_to_png(&png_with_old_metadata, new_json)
            .expect("rewrite metadata");
        let decoded = read_character_data_from_png(&rewritten).expect("read new metadata");
        let parsed: Value = serde_json::from_str(&decoded).expect("valid new json");
        assert_eq!(parsed.get("chat").and_then(Value::as_str), Some("new-chat"));

        let text_chunks = read_text_chunks_from_png(&rewritten).expect("read text metadata");
        let character_chunks_count = text_chunks
            .iter()
            .filter(|chunk| {
                chunk.keyword.eq_ignore_ascii_case("chara")
                    || chunk.keyword.eq_ignore_ascii_case("ccv3")
            })
            .count();
        assert_eq!(character_chunks_count, 2);
    }

    #[test]
    fn read_prefers_first_duplicate_metadata_chunk() {
        let base_png = build_minimal_png();
        let old_json =
            r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina","chat":"old-chat"}"#;
        let new_json =
            r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina","chat":"new-chat"}"#;

        let old_payload = encode_base64(old_json);
        let new_payload = encode_base64(new_json);

        let mut first_chunk = Vec::new();
        write_text_chunk(&mut first_chunk, "chara", &old_payload);
        let mut second_chunk = Vec::new();
        write_text_chunk(&mut second_chunk, "chara", &new_payload);

        let png_with_duplicates =
            inject_raw_chunks_before_iend(&base_png, &[first_chunk, second_chunk]);

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
        let json = r#"{"spec":"chara_card_v2","spec_version":"2.0","name":"Seraphina"}"#;
        let encoded = encode_base64(json);

        let png_with_itxt =
            inject_raw_chunks_before_iend(&base_png, &[build_itxt_chunk("ccv3", &encoded)]);

        let parsed = read_character_data_from_png(&png_with_itxt).expect("read should succeed");
        let parsed_json: Value = serde_json::from_str(&parsed).expect("valid json");

        assert_eq!(
            parsed_json.get("name").and_then(Value::as_str),
            Some("Seraphina")
        );
    }
}
