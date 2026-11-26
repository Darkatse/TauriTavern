use crate::errors::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiliconFlowEndpoint {
    Global,
    China,
}

impl SiliconFlowEndpoint {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "global" | "com" | "https://api.siliconflow.com/v1" => Ok(Self::Global),
            "cn" | "china" | "https://api.siliconflow.cn/v1" => Ok(Self::China),
            other => Err(DomainError::InvalidData(format!(
                "Unsupported SiliconFlow endpoint: {other}"
            ))),
        }
    }

    pub const fn base_url(self) -> &'static str {
        match self {
            Self::Global => "https://api.siliconflow.com/v1",
            Self::China => "https://api.siliconflow.cn/v1",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_endpoints_are_rejected() {
        assert!(matches!(
            SiliconFlowEndpoint::parse("edge"),
            Err(DomainError::InvalidData(_))
        ));
    }
}
