use std::io::{Cursor, Read, Write};
use std::collections::HashMap;
use image::{ImageFormat, GenericImageView};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Serialize, de::DeserializeOwned};
use crc32fast::Hasher;
use crate::domain::errors::DomainError;
use crate::infrastructure::logging::logger;
use crate::domain::repositories::character_repository::ImageCrop;

/// PNG chunk names used for character data
const CHUNK_NAME_V2: &str = "chara";
const CHUNK_NAME_V3: &str = "ccv3";

/// PNG signature bytes
const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Represents a PNG chunk
#[derive(Debug, Clone)]
pub struct PngChunk {
    pub name: [u8; 4],
    pub data: Vec<u8>,
}

impl PngChunk {
    /// Create a new PNG chunk
    pub fn new(name: &str, data: Vec<u8>) -> Self {
        let mut name_bytes = [0; 4];
        for (i, b) in name.bytes().take(4).enumerate() {
            name_bytes[i] = b;
        }

        Self {
            name: name_bytes,
            data,
        }
    }

    /// Get the chunk name as a string
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(&self.name).to_string()
    }

    /// Calculate the CRC for this chunk
    pub fn calculate_crc(&self) -> u32 {
        let mut hasher = Hasher::new();
        hasher.update(&self.name);
        hasher.update(&self.data);
        hasher.finalize()
    }
}

/// Represents a PNG text chunk
#[derive(Debug, Clone)]
pub struct TextChunk {
    pub keyword: String,
    pub text: String,
}

/// Reads character data from a PNG image
///
/// This function extracts character data from PNG metadata chunks.
/// It supports both V2 (chara) and V3 (ccv3) formats, with V3 taking precedence.
///
/// # Arguments
///
/// * `image_data` - The PNG image data as a byte vector
///
/// # Returns
///
/// * `Ok(String)` - The character data as a JSON string
/// * `Err(DomainError)` - If the image cannot be read or contains no character data
pub fn read_character_data_from_png(image_data: &[u8]) -> Result<String, DomainError> {
    tracing::debug!("Reading character data from PNG");

    // Extract text chunks from the PNG
    let chunks = extract_chunks(image_data)?;
    let text_chunks = extract_text_chunks_from_chunks(&chunks)?;

    if text_chunks.is_empty() {
        return Err(DomainError::InvalidData("PNG metadata does not contain any text chunks".to_string()));
    }

    // First try to find V3 format (ccv3)
    if let Some(chunk) = text_chunks.iter().find(|c| c.keyword.to_lowercase() == CHUNK_NAME_V3) {
        let decoded = decode_base64(&chunk.text)?;
        return Ok(decoded);
    }

    // Then try V2 format (chara)
    if let Some(chunk) = text_chunks.iter().find(|c| c.keyword.to_lowercase() == CHUNK_NAME_V2) {
        let decoded = decode_base64(&chunk.text)?;
        return Ok(decoded);
    }

    Err(DomainError::InvalidData("PNG metadata does not contain character data".to_string()))
}

/// Writes character data to a PNG image
///
/// This function embeds character data into PNG metadata chunks.
/// It writes both V2 (chara) and V3 (ccv3) formats for compatibility.
///
/// # Arguments
///
/// * `image_data` - The original PNG image data
/// * `character_data` - The character data as a JSON string
///
/// # Returns
///
/// * `Ok(Vec<u8>)` - The new PNG image data with embedded character data
/// * `Err(DomainError)` - If the image cannot be written or the data cannot be embedded
pub fn write_character_data_to_png(image_data: &[u8], character_data: &str) -> Result<Vec<u8>, DomainError> {
    tracing::debug!("Writing character data to PNG");

    // Extract existing chunks
    let mut chunks = extract_chunks(image_data)?;

    // Remove existing character data chunks
    chunks.retain(|c| {
        let name = c.name_str().to_lowercase();
        name != CHUNK_NAME_V2 && name != CHUNK_NAME_V3
    });

    // Add V2 chunk
    let base64_data = encode_base64(character_data)?;
    let v2_chunk_data = create_text_chunk_data(CHUNK_NAME_V2, &base64_data);
    chunks.insert(chunks.len() - 1, PngChunk::new("tEXt", v2_chunk_data));

    // Try to add V3 chunk
    if let Ok(mut v3_data) = serde_json::from_str::<serde_json::Value>(character_data) {
        if let Some(obj) = v3_data.as_object_mut() {
            obj.insert("spec".to_string(), serde_json::Value::String("chara_card_v3".to_string()));
            obj.insert("spec_version".to_string(), serde_json::Value::String("3.0".to_string()));

            if let Ok(v3_json) = serde_json::to_string(&v3_data) {
                let v3_base64 = encode_base64(&v3_json)?;
                let v3_chunk_data = create_text_chunk_data(CHUNK_NAME_V3, &v3_base64);
                chunks.insert(chunks.len() - 1, PngChunk::new("tEXt", v3_chunk_data));
            }
        }
    }

    // Encode the chunks back to a PNG
    let output = encode_chunks(&chunks)?;

    Ok(output)
}

/// Create text chunk data
fn create_text_chunk_data(keyword: &str, text: &str) -> Vec<u8> {
    let mut data = Vec::new();

    // Add keyword
    data.extend_from_slice(keyword.as_bytes());

    // Add null separator
    data.push(0);

    // Add text
    data.extend_from_slice(text.as_bytes());

    data
}

/// Extract PNG chunks from image data
pub fn extract_chunks(image_data: &[u8]) -> Result<Vec<PngChunk>, DomainError> {
    let mut chunks = Vec::new();
    let mut cursor = Cursor::new(image_data);

    // Check PNG signature
    let mut signature = [0; 8];
    cursor.read_exact(&mut signature).map_err(|e| {
        DomainError::InvalidData(format!("Failed to read PNG signature: {}", e))
    })?;

    if signature != PNG_SIGNATURE {
        return Err(DomainError::InvalidData("Invalid PNG signature".to_string()));
    }

    // Read chunks
    loop {
        // Read chunk length
        let mut length_bytes = [0; 4];
        if cursor.read_exact(&mut length_bytes).is_err() {
            break; // End of file
        }

        let length = u32::from_be_bytes(length_bytes) as usize;

        // Read chunk type
        let mut chunk_type = [0; 4];
        cursor.read_exact(&mut chunk_type).map_err(|e| {
            DomainError::InvalidData(format!("Failed to read chunk type: {}", e))
        })?;

        // Read chunk data
        let mut data = vec![0; length];
        cursor.read_exact(&mut data).map_err(|e| {
            DomainError::InvalidData(format!("Failed to read chunk data: {}", e))
        })?;

        // Skip CRC
        let mut crc = [0; 4];
        cursor.read_exact(&mut crc).map_err(|e| {
            DomainError::InvalidData(format!("Failed to read chunk CRC: {}", e))
        })?;

        // Add chunk to list
        chunks.push(PngChunk {
            name: chunk_type,
            data,
        });

        // Check if this is the IEND chunk
        if chunk_type == *b"IEND" {
            break;
        }
    }

    Ok(chunks)
}

/// Encode PNG chunks to image data
pub fn encode_chunks(chunks: &[PngChunk]) -> Result<Vec<u8>, DomainError> {
    let mut output = Vec::new();

    // Write PNG signature
    output.extend_from_slice(&PNG_SIGNATURE);

    // Write chunks
    for chunk in chunks {
        // Write chunk length
        let length = chunk.data.len() as u32;
        output.extend_from_slice(&length.to_be_bytes());

        // Write chunk type
        output.extend_from_slice(&chunk.name);

        // Write chunk data
        output.extend_from_slice(&chunk.data);

        // Calculate and write CRC
        let crc = chunk.calculate_crc();
        output.extend_from_slice(&crc.to_be_bytes());
    }

    Ok(output)
}

/// Extract text chunks from PNG chunks
fn extract_text_chunks_from_chunks(chunks: &[PngChunk]) -> Result<Vec<TextChunk>, DomainError> {
    let mut text_chunks = Vec::new();

    for chunk in chunks {
        if chunk.name_str() == "tEXt" {
            // Find the null separator
            let null_pos = chunk.data.iter().position(|&b| b == 0);

            if let Some(pos) = null_pos {
                let keyword = String::from_utf8_lossy(&chunk.data[0..pos]).to_string();
                let text = String::from_utf8_lossy(&chunk.data[pos+1..]).to_string();

                text_chunks.push(TextChunk {
                    keyword,
                    text,
                });
            }
        }
    }

    Ok(text_chunks)
}

/// Encodes a string to base64
fn encode_base64(data: &str) -> Result<String, DomainError> {
    Ok(BASE64.encode(data.as_bytes()))
}

/// Decodes a base64 string
fn decode_base64(data: &str) -> Result<String, DomainError> {
    let bytes = BASE64.decode(data)
        .map_err(|e| DomainError::InvalidData(format!("Failed to decode base64: {}", e)))?;

    String::from_utf8(bytes)
        .map_err(|e| DomainError::InvalidData(format!("Failed to convert from UTF-8: {}", e)))
}

/// Parses a character from a PNG file
///
/// # Arguments
///
/// * `image_data` - The PNG image data
///
/// # Returns
///
/// * `Ok(T)` - The parsed character
/// * `Err(DomainError)` - If the character cannot be parsed
pub fn parse_character_from_png<T: DeserializeOwned>(image_data: &[u8]) -> Result<T, DomainError> {
    let json_data = read_character_data_from_png(image_data)?;

    serde_json::from_str(&json_data)
        .map_err(|e| DomainError::InvalidData(format!("Failed to parse character data: {}", e)))
}

/// Writes a character to a PNG file
///
/// # Arguments
///
/// * `image_data` - The original PNG image data
/// * `character` - The character to write
///
/// # Returns
///
/// * `Ok(Vec<u8>)` - The new PNG image data with embedded character
/// * `Err(DomainError)` - If the character cannot be written
pub fn write_character_to_png<T: Serialize>(image_data: &[u8], character: &T) -> Result<Vec<u8>, DomainError> {
    let json_data = serde_json::to_string(character)
        .map_err(|e| DomainError::InvalidData(format!("Failed to serialize character: {}", e)))?;

    write_character_data_to_png(image_data, &json_data)
}

/// Process an image for use as a character avatar
///
/// # Arguments
///
/// * `image_data` - The image data
/// * `crop` - Optional crop parameters
///
/// # Returns
///
/// * `Ok(Vec<u8>)` - The processed image data
/// * `Err(DomainError)` - If the image cannot be processed
pub async fn process_avatar_image(image_data: &[u8], crop: Option<ImageCrop>) -> Result<Vec<u8>, DomainError> {
    tracing::debug!("Processing avatar image");

    // Load the image
    let mut img = image::load_from_memory(image_data)
        .map_err(|e| DomainError::InvalidData(format!("Failed to load image: {}", e)))?;

    // Apply crop if defined
    if let Some(crop_params) = crop {
        if crop_params.x >= 0 && crop_params.y >= 0 &&
           crop_params.width > 0 && crop_params.height > 0 &&
           (crop_params.x as u32 + crop_params.width as u32) <= img.width() &&
           (crop_params.y as u32 + crop_params.height as u32) <= img.height() {

            img = img.crop(
                crop_params.x as u32,
                crop_params.y as u32,
                crop_params.width as u32,
                crop_params.height as u32
            );

            // Apply standard resize if requested
            if crop_params.want_resize {
                // Use the standard avatar dimensions from SillyTavern
                const AVATAR_WIDTH: u32 = 400;
                const AVATAR_HEIGHT: u32 = 600;

                img = img.resize_to_fill(AVATAR_WIDTH, AVATAR_HEIGHT, image::imageops::FilterType::Lanczos3);
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
