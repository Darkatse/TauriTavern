use serde::Serialize;
use tt_domain::models::upstream_failure::UpstreamFailure;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GenerationStreamEventDto {
    Chunk {
        seq: u64,
        data: String,
    },
    Done {
        seq: u64,
    },
    Error {
        seq: u64,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<UpstreamFailure>,
    },
}

impl GenerationStreamEventDto {
    pub(crate) fn seq(&self) -> u64 {
        match self {
            Self::Chunk { seq, .. } | Self::Done { seq } | Self::Error { seq, .. } => *seq,
        }
    }

    pub(crate) fn buffered_bytes(&self) -> usize {
        match self {
            Self::Chunk { data, .. } => data.len(),
            Self::Done { .. } | Self::Error { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStreamStatusDto {
    Running,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GenerationStreamReadResultDto {
    pub events: Vec<GenerationStreamEventDto>,
    pub status: GenerationStreamStatusDto,
}
