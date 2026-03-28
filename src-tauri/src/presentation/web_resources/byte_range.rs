#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeHeaderError {
    Invalid,
    Unsatisfiable,
}

pub fn parse_single_range_header(
    value: &str,
    total_size: u64,
) -> Result<ByteRange, RangeHeaderError> {
    let value = value.trim();
    let Some(value) = value.strip_prefix("bytes=") else {
        return Err(RangeHeaderError::Invalid);
    };

    if value.contains(',') {
        return Err(RangeHeaderError::Invalid);
    }

    let Some((start_raw, end_raw)) = value.split_once('-') else {
        return Err(RangeHeaderError::Invalid);
    };

    if total_size == 0 {
        return Err(RangeHeaderError::Unsatisfiable);
    }

    let last = total_size - 1;

    if start_raw.is_empty() {
        let suffix_len: u64 = end_raw.parse().map_err(|_| RangeHeaderError::Invalid)?;
        if suffix_len == 0 {
            return Err(RangeHeaderError::Unsatisfiable);
        }

        let start = if suffix_len >= total_size {
            0
        } else {
            total_size - suffix_len
        };

        return Ok(ByteRange { start, end: last });
    }

    let start: u64 = start_raw.parse().map_err(|_| RangeHeaderError::Invalid)?;
    if start >= total_size {
        return Err(RangeHeaderError::Unsatisfiable);
    }

    let end = if end_raw.is_empty() {
        last
    } else {
        let parsed_end: u64 = end_raw.parse().map_err(|_| RangeHeaderError::Invalid)?;
        parsed_end.min(last)
    };

    if end < start {
        return Err(RangeHeaderError::Unsatisfiable);
    }

    Ok(ByteRange { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_ended_range() {
        let range = parse_single_range_header("bytes=1-", 4).expect("parse");
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 3);
        assert_eq!(range.len(), 3);
    }

    #[test]
    fn parses_explicit_range() {
        let range = parse_single_range_header("bytes=1-2", 4).expect("parse");
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 2);
        assert_eq!(range.len(), 2);
    }

    #[test]
    fn clamps_end_to_total_size() {
        let range = parse_single_range_header("bytes=1-999", 4).expect("parse");
        assert_eq!(range.start, 1);
        assert_eq!(range.end, 3);
    }

    #[test]
    fn parses_suffix_range() {
        let range = parse_single_range_header("bytes=-2", 4).expect("parse");
        assert_eq!(range.start, 2);
        assert_eq!(range.end, 3);
    }

    #[test]
    fn rejects_multiple_ranges() {
        let result = parse_single_range_header("bytes=0-0,2-2", 4);
        assert_eq!(result, Err(RangeHeaderError::Invalid));
    }

    #[test]
    fn rejects_unsatisfiable_range_start_beyond_length() {
        let result = parse_single_range_header("bytes=4-", 4);
        assert_eq!(result, Err(RangeHeaderError::Unsatisfiable));
    }

    #[test]
    fn rejects_unsatisfiable_suffix_zero() {
        let result = parse_single_range_header("bytes=-0", 4);
        assert_eq!(result, Err(RangeHeaderError::Unsatisfiable));
    }
}
