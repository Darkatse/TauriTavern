use crc32fast::Hasher;
use flate2::read::ZlibDecoder;
use std::io::{Read, Seek, SeekFrom};
use tt_domain::errors::DomainError;

pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
pub const CHUNK_TYPE_TEXT: [u8; 4] = *b"tEXt";
pub const CHUNK_TYPE_ZTXT: [u8; 4] = *b"zTXt";
pub const CHUNK_TYPE_ITXT: [u8; 4] = *b"iTXt";
pub const CHUNK_TYPE_IEND: [u8; 4] = *b"IEND";

/// Logical text entry parsed from PNG metadata (tEXt/zTXt/iTXt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub keyword: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PngChunkRef<'a> {
    pub chunk_type: [u8; 4],
    pub data: &'a [u8],
    pub raw: &'a [u8],
}

pub fn ensure_png_signature(image_data: &[u8]) -> Result<(), DomainError> {
    if image_data.len() < PNG_SIGNATURE.len() || image_data[..PNG_SIGNATURE.len()] != PNG_SIGNATURE
    {
        return Err(DomainError::InvalidData(
            "Failed to read PNG header: invalid PNG signature".to_string(),
        ));
    }

    Ok(())
}

pub fn read_next_png_chunk<'a>(
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

    let data_end = offset.checked_add(length).ok_or_else(|| {
        DomainError::InvalidData("Failed to parse PNG metadata: chunk too large".to_string())
    })?;
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

fn split_keyword<'a>(
    data: &'a [u8],
    chunk_name: &str,
) -> Result<(&'a [u8], &'a [u8]), DomainError> {
    let Some(nul) = data.iter().position(|&byte| byte == 0) else {
        return Err(DomainError::InvalidData(format!(
            "Failed to parse PNG metadata: invalid {} chunk",
            chunk_name
        )));
    };

    Ok((&data[..nul], &data[nul + 1..]))
}

pub fn parse_text_chunk(
    chunk_type: [u8; 4],
    data: &[u8],
) -> Result<Option<TextChunk>, DomainError> {
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

pub fn write_chunk(output: &mut Vec<u8>, chunk_type: [u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(&chunk_type);
    output.extend_from_slice(data);

    let mut hasher = Hasher::new();
    hasher.update(&chunk_type);
    hasher.update(data);
    output.extend_from_slice(&hasher.finalize().to_be_bytes());
}

pub fn write_text_chunk(output: &mut Vec<u8>, keyword: &str, text: &str) {
    let mut data = Vec::with_capacity(keyword.len() + 1 + text.len());
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(text.as_bytes());

    write_chunk(output, CHUNK_TYPE_TEXT, &data);
}

pub fn text_chunk_keyword(chunk_type: [u8; 4], data: &[u8]) -> Result<Option<&[u8]>, DomainError> {
    let keyword = match chunk_type {
        CHUNK_TYPE_TEXT => split_keyword(data, "tEXt")?.0,
        CHUNK_TYPE_ZTXT => split_keyword(data, "zTXt")?.0,
        CHUNK_TYPE_ITXT => split_keyword(data, "iTXt")?.0,
        _ => return Ok(None),
    };

    Ok(Some(keyword))
}

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

/// Read the first available field in priority order, stopping as soon as the first priority is found.
/// Image chunks and lower-priority text are skipped without decoding them.
pub fn read_preferred_text_chunk(
    mut reader: impl Read + Seek,
    keywords: &[&str],
) -> Result<Option<TextChunk>, DomainError> {
    let length = reader.seek(SeekFrom::End(0)).map_err(io_error)?;
    reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut signature = [0; 8];
    reader.read_exact(&mut signature).map_err(io_error)?;
    ensure_png_signature(&signature)?;
    let mut selected = None;
    let mut priority = keywords.len();
    let mut offset = 8u64;
    while offset + 8 <= length {
        let mut header = [0; 8];
        reader.read_exact(&mut header).map_err(io_error)?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as u64;
        let chunk_type = header[4..].try_into().unwrap();
        let end = offset + 12 + size;
        if end > length {
            return Err(DomainError::InvalidData("truncated PNG chunk".into()));
        }
        if chunk_type == CHUNK_TYPE_IEND {
            break;
        }
        if matches!(
            chunk_type,
            CHUNK_TYPE_TEXT | CHUNK_TYPE_ITXT | CHUNK_TYPE_ZTXT
        ) {
            let mut data = vec![0; size as usize];
            reader.read_exact(&mut data).map_err(io_error)?;
            if let Some(keyword) = text_chunk_keyword(chunk_type, &data)?
                && let Some(index) = keywords
                    .iter()
                    .position(|key| keyword.eq_ignore_ascii_case(key.as_bytes()))
                && index < priority
            {
                selected = parse_text_chunk(chunk_type, &data)?;
                if index == 0 {
                    return Ok(selected);
                }
                priority = index;
            }
        }
        reader.seek(SeekFrom::Start(end)).map_err(io_error)?;
        offset = end;
    }
    Ok(selected)
}

fn io_error(error: std::io::Error) -> DomainError {
    DomainError::InternalError(error.to_string())
}

/// Replace selected text fields while preserving every other PNG chunk verbatim.
pub fn replace_text_chunks(
    image: &[u8],
    keywords: &[&str],
    replacement: &[u8],
) -> Result<Vec<u8>, DomainError> {
    ensure_png_signature(image)?;
    let mut output = Vec::with_capacity(image.len() + replacement.len());
    output.extend_from_slice(&PNG_SIGNATURE);
    let mut offset = PNG_SIGNATURE.len();
    while let Some(chunk) = read_next_png_chunk(image, &mut offset)? {
        if chunk.chunk_type == CHUNK_TYPE_IEND {
            output.extend_from_slice(replacement);
            output.extend_from_slice(chunk.raw);
            return Ok(output);
        }
        if text_chunk_keyword(chunk.chunk_type, chunk.data)?.is_some_and(|keyword| {
            keywords
                .iter()
                .any(|key| keyword.eq_ignore_ascii_case(key.as_bytes()))
        }) {
            continue;
        }
        output.extend_from_slice(chunk.raw);
    }
    Err(DomainError::InvalidData("PNG is missing IEND".into()))
}

pub fn write_utf8_text_chunk(output: &mut Vec<u8>, keyword: &str, text: &str) {
    let mut data = Vec::with_capacity(keyword.len() + text.len() + 5);
    data.extend_from_slice(keyword.as_bytes());
    data.extend_from_slice(&[0; 5]); // Uncompressed iTXt, no language or translated keyword.
    data.extend_from_slice(text.as_bytes());
    write_chunk(output, CHUNK_TYPE_ITXT, &data);
}
