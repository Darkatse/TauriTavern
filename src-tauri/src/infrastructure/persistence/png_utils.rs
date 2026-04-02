use crate::domain::errors::DomainError;
use crate::domain::repositories::character_repository::ImageCrop;
use crate::infrastructure::logging::logger;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use crc32fast::Hasher;
use flate2::read::ZlibDecoder;
use image::ImageFormat;
use std::io::{Cursor, Read};

/// PNG text keys used for character data.
const CHUNK_NAME_V2: &str = "chara";
const CHUNK_NAME_V3: &str = "ccv3";

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const CHUNK_TYPE_TEXT: [u8; 4] = *b"tEXt";
const CHUNK_TYPE_ZTXT: [u8; 4] = *b"zTXt";
const CHUNK_TYPE_ITXT: [u8; 4] = *b"iTXt";
const CHUNK_TYPE_IEND: [u8; 4] = *b"IEND";

/// Logical text entry parsed from PNG metadata (tEXt/zTXt/iTXt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub keyword: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy)]
struct PngChunkRef<'a> {
    chunk_type: [u8; 4],
    data: &'a [u8],
    raw: &'a [u8],
}

fn ensure_png_signature(image_data: &[u8]) -> Result<(), DomainError> {
    if image_data.len() < PNG_SIGNATURE.len() || image_data[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(DomainError::InvalidData(
            "Failed to read PNG header: invalid PNG signature".to_string(),
        ));
    }

    Ok(())
}

fn read_next_png_chunk<'a>(
    image_data: &'a [u8],
    offset: &mut usize,
) -> Result<Option<PngChunkRef<'a>>, DomainError> {
    if *offset + 8 > image_data.len() {
        return Ok(None);
    }

    let start = *offset;

    let length = u32::from_be_bytes(
        image_data[*offset..*offset + 4]
            .try_into()
            .expect("slice has 4 bytes"),
    ) as usize;
    let chunk_type: [u8; 4] = image_data[*offset + 4..*offset + 8]
        .try_into()
        .expect("slice has 4 bytes");

    *offset += 8;

    let data_end = offset
        .checked_add(length)
        .ok_or_else(|| DomainError::InvalidData("Failed to parse PNG metadata: chunk too large".to_string()))?;
    let crc_end = data_end.checked_add(4).ok_or_else(|| {
        DomainError::InvalidData("Failed to parse PNG metadata: chunk too large".to_string())
    })?;

    if crc_end > image_data.len() {
        return Err(DomainError::InvalidData(
            "Failed to parse PNG metadata: truncated PNG chunk".to_string(),
        ));
    }

    let data = &image_data[*offset..data_end];
    let raw = &image_data[start..crc_end];
    *offset = crc_end;

    Ok(Some(PngChunkRef {
        chunk_type,
        data,
        raw,
    }))
}

fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

fn split_keyword<'a>(data: &'a [u8], chunk_name: &str) -> Result<(&'a [u8], &'a [u8]), DomainError> {
    let Some(nul) = data.iter().position(|&byte| byte == 0) else {
        return Err(DomainError::InvalidData(format!(
            "Failed to parse PNG metadata: invalid {} chunk",
            chunk_name
        )));
    };

    Ok((&data[..nul], &data[nul + 1..]))
}

fn parse_text_chunk(chunk_type: [u8; 4], data: &[u8]) -> Result<Option<TextChunk>, DomainError> {
    if chunk_type == CHUNK_TYPE_TEXT {
        let (keyword, text) = split_keyword(data, "tEXt")?;
        return Ok(Some(TextChunk {
            keyword: decode_latin1(keyword),
            text: decode_latin1(text),
        }));
    }

    if chunk_type == CHUNK_TYPE_ZTXT {
        let (keyword, rest) = split_keyword(data, "zTXt")?;
        let Some((&compression_method, compressed_text)) = rest.split_first() else {
            return Err(DomainError::InvalidData(
                "Failed to decode zTXt metadata: missing compression method".to_string(),
            ));
        };

        if compression_method != 0 {
            return Err(DomainError::InvalidData(
                "Failed to decode zTXt metadata: unsupported compression method".to_string(),
            ));
        }

        let mut decoder = ZlibDecoder::new(compressed_text);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).map_err(|error| {
            DomainError::InvalidData(format!("Failed to decode zTXt metadata: {}", error))
        })?;

        return Ok(Some(TextChunk {
            keyword: decode_latin1(keyword),
            text: decode_latin1(&decoded),
        }));
    }

    if chunk_type == CHUNK_TYPE_ITXT {
        let (keyword, rest) = split_keyword(data, "iTXt")?;
        if rest.len() < 2 {
            return Err(DomainError::InvalidData(
                "Failed to decode iTXt metadata: missing compression fields".to_string(),
            ));
        }

        let compression_flag = rest[0];
        let compression_method = rest[1];
        let mut cursor = &rest[2..];

        let (_, after_language) = split_keyword(cursor, "iTXt")?;
        cursor = after_language;
        let (_, after_translated) = split_keyword(cursor, "iTXt")?;
        cursor = after_translated;

        let text_bytes = if compression_flag == 0 {
            cursor.to_vec()
        } else if compression_flag == 1 {
            if compression_method != 0 {
                return Err(DomainError::InvalidData(
                    "Failed to decode iTXt metadata: unsupported compression method".to_string(),
                ));
            }

            let mut decoder = ZlibDecoder::new(cursor);
            let mut decoded = Vec::new();
            decoder.read_to_end(&mut decoded).map_err(|error| {
                DomainError::InvalidData(format!("Failed to decode iTXt metadata: {}", error))
            })?;
            decoded
        } else {
            return Err(DomainError::InvalidData(
                "Failed to decode iTXt metadata: invalid compression flag".to_string(),
            ));
        };

        let text = String::from_utf8(text_bytes).map_err(|error| {
            DomainError::InvalidData(format!("Failed to decode iTXt metadata: {}", error))
        })?;

        return Ok(Some(TextChunk {
            keyword: decode_latin1(keyword),
            text,
        }));
    }

    Ok(None)
}

fn write_chunk(output: &mut Vec<u8>, chunk_type: [u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(&chunk_type);
    output.extend_from_slice(data);

    let mut hasher = Hasher::new();
    hasher.update(&chunk_type);
    hasher.update(data);
    output.extend_from_slice(&hasher.finalize().to_be_bytes());
}

fn write_text_chunk(output: &mut Vec<u8>, keyword: &str, text: &str) {
    let mut data = Vec::with_capacity(keyword.len() + 1 + text.len());
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(text.as_bytes());

    write_chunk(output, CHUNK_TYPE_TEXT, &data);
}

/// Reads all text metadata chunks from a PNG image.
///
/// This includes `tEXt`, `zTXt`, and `iTXt` chunks.
pub fn read_text_chunks_from_png(image_data: &[u8]) -> Result<Vec<TextChunk>, DomainError> {
    ensure_png_signature(image_data)?;

    let mut chunks = Vec::new();
    let mut offset = PNG_SIGNATURE.len();

    while let Some(chunk) = read_next_png_chunk(image_data, &mut offset)? {
        if let Some(text_chunk) = parse_text_chunk(chunk.chunk_type, chunk.data)? {
            chunks.push(text_chunk);
        }

        if chunk.chunk_type == CHUNK_TYPE_IEND {
            break;
        }
    }

    Ok(chunks)
}

/// Reads character data from PNG metadata.
///
/// It prefers V3 (`ccv3`) and falls back to V2 (`chara`).
pub fn read_character_data_from_png(image_data: &[u8]) -> Result<String, DomainError> {
    tracing::debug!("Reading character data from PNG");

    ensure_png_signature(image_data)?;

    let mut saw_text_chunk = false;
    let mut v2_payload: Option<String> = None;
    let mut offset = PNG_SIGNATURE.len();

    while let Some(chunk) = read_next_png_chunk(image_data, &mut offset)? {
        if chunk.chunk_type == CHUNK_TYPE_IEND {
            break;
        }

        if chunk.chunk_type == CHUNK_TYPE_TEXT {
            saw_text_chunk = true;
            let (keyword, text) = split_keyword(chunk.data, "tEXt")?;

            if keyword.eq_ignore_ascii_case(CHUNK_NAME_V3.as_bytes()) {
                return decode_base64(&decode_latin1(text));
            }

            if keyword.eq_ignore_ascii_case(CHUNK_NAME_V2.as_bytes()) && v2_payload.is_none() {
                v2_payload = Some(decode_latin1(text));
            }

            continue;
        }

        if chunk.chunk_type == CHUNK_TYPE_ZTXT {
            saw_text_chunk = true;
            let (keyword, _) = split_keyword(chunk.data, "zTXt")?;

            if !keyword.eq_ignore_ascii_case(CHUNK_NAME_V3.as_bytes())
                && !keyword.eq_ignore_ascii_case(CHUNK_NAME_V2.as_bytes())
            {
                continue;
            }

            let Some(text_chunk) = parse_text_chunk(chunk.chunk_type, chunk.data)? else {
                continue;
            };

            if text_chunk.keyword.eq_ignore_ascii_case(CHUNK_NAME_V3) {
                return decode_base64(&text_chunk.text);
            }

            if text_chunk.keyword.eq_ignore_ascii_case(CHUNK_NAME_V2) && v2_payload.is_none() {
                v2_payload = Some(text_chunk.text);
            }

            continue;
        }

        if chunk.chunk_type == CHUNK_TYPE_ITXT {
            saw_text_chunk = true;
            let (keyword, _) = split_keyword(chunk.data, "iTXt")?;

            if !keyword.eq_ignore_ascii_case(CHUNK_NAME_V3.as_bytes())
                && !keyword.eq_ignore_ascii_case(CHUNK_NAME_V2.as_bytes())
            {
                continue;
            }

            let Some(text_chunk) = parse_text_chunk(chunk.chunk_type, chunk.data)? else {
                continue;
            };

            if text_chunk.keyword.eq_ignore_ascii_case(CHUNK_NAME_V3) {
                return decode_base64(&text_chunk.text);
            }

            if text_chunk.keyword.eq_ignore_ascii_case(CHUNK_NAME_V2) && v2_payload.is_none() {
                v2_payload = Some(text_chunk.text);
            }
        }
    }

    if let Some(payload) = v2_payload {
        return decode_base64(&payload);
    }

    if !saw_text_chunk {
        return Err(DomainError::InvalidData(
            "PNG metadata does not contain any text chunks".to_string(),
        ));
    }

    Err(DomainError::InvalidData(
        "PNG metadata does not contain character data".to_string(),
    ))
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

    ensure_png_signature(image_data)?;

    let v2_payload = encode_base64(character_data);
    let v3_payload = build_v3_payload(character_data)?;

    let extra_capacity = v2_payload.len() + v3_payload.as_ref().map(String::len).unwrap_or(0) + 128;
    let mut output = Vec::with_capacity(image_data.len() + extra_capacity);
    output.extend_from_slice(&PNG_SIGNATURE);

    let mut offset = PNG_SIGNATURE.len();
    let mut wrote_iend = false;

    while let Some(chunk) = read_next_png_chunk(image_data, &mut offset)? {
        if chunk.chunk_type == CHUNK_TYPE_IEND {
            write_text_chunk(&mut output, CHUNK_NAME_V2, &v2_payload);
            if let Some(v3_payload) = &v3_payload {
                write_text_chunk(&mut output, CHUNK_NAME_V3, v3_payload);
            }

            output.extend_from_slice(chunk.raw);
            wrote_iend = true;
            break;
        }

        if chunk.chunk_type == CHUNK_TYPE_TEXT {
            let (keyword, _) = split_keyword(chunk.data, "tEXt")?;
            if keyword.eq_ignore_ascii_case(CHUNK_NAME_V2.as_bytes())
                || keyword.eq_ignore_ascii_case(CHUNK_NAME_V3.as_bytes())
            {
                continue;
            }
        }

        output.extend_from_slice(chunk.raw);
    }

    if !wrote_iend {
        return Err(DomainError::InvalidData(
            "Failed to parse PNG metadata: missing IEND chunk".to_string(),
        ));
    }

    Ok(output)
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
        CHUNK_TYPE_IEND, CHUNK_TYPE_ITXT, PNG_SIGNATURE, decode_base64, encode_base64,
        read_character_data_from_png, read_next_png_chunk, read_text_chunks_from_png,
        write_character_data_to_png, write_chunk, write_text_chunk,
    };
    use image::{DynamicImage, ImageFormat, RgbaImage};
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

        let mut itxt_data = Vec::new();
        itxt_data.extend_from_slice(b"ccv3");
        itxt_data.push(0);
        itxt_data.push(0);
        itxt_data.push(0);
        itxt_data.push(0);
        itxt_data.push(0);
        itxt_data.extend_from_slice(encoded.as_bytes());

        let mut itxt_chunk = Vec::new();
        write_chunk(&mut itxt_chunk, CHUNK_TYPE_ITXT, &itxt_data);

        let png_with_itxt = inject_raw_chunks_before_iend(&base_png, &[itxt_chunk]);

        let parsed = read_character_data_from_png(&png_with_itxt).expect("read should succeed");
        let parsed_json: Value = serde_json::from_str(&parsed).expect("valid json");

        assert_eq!(
            parsed_json.get("name").and_then(Value::as_str),
            Some("Seraphina")
        );
    }
}
