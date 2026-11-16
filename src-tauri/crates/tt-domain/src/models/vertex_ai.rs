use crate::errors::DomainError;
use crate::models::endpoint_url::HostLabel;

pub const VERTEX_AI_GLOBAL_HOST: &str = "https://aiplatform.googleapis.com";
const VERTEX_AI_DEFAULT_REGION: &str = "us-central1";

pub fn vertex_ai_region(raw: &str) -> Result<HostLabel, DomainError> {
    let raw = raw.trim();
    let region = if raw.is_empty() {
        VERTEX_AI_DEFAULT_REGION.to_string()
    } else {
        raw.to_ascii_lowercase()
    };
    HostLabel::parse("Vertex AI region", &region)
}

pub fn vertex_ai_host(region: &HostLabel) -> String {
    match region.as_str() {
        "global" => VERTEX_AI_GLOBAL_HOST.to_string(),
        "us" | "eu" => format!("https://aiplatform.{region}.rep.googleapis.com"),
        _ => format!("https://{region}-aiplatform.googleapis.com"),
    }
}
