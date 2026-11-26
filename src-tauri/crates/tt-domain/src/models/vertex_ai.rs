use serde::Deserialize;

use crate::errors::DomainError;
use crate::models::endpoint_url::{HostLabel, append_endpoint_segments};
use crate::models::secret::SecretKeys;

pub const VERTEX_AI_GLOBAL_HOST: &str = "https://aiplatform.googleapis.com";
const VERTEX_AI_DEFAULT_REGION: &str = "us-central1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexAiAuthMode {
    Express,
    Full,
}

impl VertexAiAuthMode {
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "express" => Ok(Self::Express),
            "full" => Ok(Self::Full),
            other => Err(DomainError::InvalidData(format!(
                "Unsupported Vertex AI authentication mode: {other}"
            ))),
        }
    }

    /// The secret that holds this mode's credential.
    pub const fn secret_key(self) -> &'static str {
        match self {
            Self::Express => SecretKeys::VERTEXAI,
            Self::Full => SecretKeys::VERTEXAI_SERVICE_ACCOUNT,
        }
    }
}

/// The user's Vertex AI connection settings, before the credential is read.
#[derive(Debug, Clone)]
pub struct VertexAiSettings {
    auth_mode: VertexAiAuthMode,
    region: HostLabel,
    express_project_id: Option<String>,
}

impl VertexAiSettings {
    pub fn parse(
        auth_mode: &str,
        region: &str,
        express_project_id: &str,
    ) -> Result<Self, DomainError> {
        let auth_mode = VertexAiAuthMode::parse(auth_mode)?;
        let express_project_id = express_project_id.trim();
        Ok(Self {
            auth_mode,
            region: parse_region(region)?,
            express_project_id: (auth_mode == VertexAiAuthMode::Express
                && !express_project_id.is_empty())
            .then(|| express_project_id.to_string()),
        })
    }

    pub const fn auth_mode(&self) -> VertexAiAuthMode {
        self.auth_mode
    }

    /// `credential` is the secret stored under `auth_mode().secret_key()`.
    pub fn connect(self, credential: String) -> Result<VertexAiConnection, DomainError> {
        let (project_id, credential) = match self.auth_mode {
            VertexAiAuthMode::Express => (
                self.express_project_id,
                VertexAiCredential::ApiKey(credential),
            ),
            VertexAiAuthMode::Full => (
                Some(service_account_project_id(&credential)?),
                VertexAiCredential::ServiceAccount(credential),
            ),
        };
        Ok(VertexAiConnection {
            base_url: base_url(&self.region, project_id.as_deref())?,
            credential,
        })
    }
}

/// Where and how to call Vertex AI; the base URL already carries project and location.
#[derive(Clone)]
pub struct VertexAiConnection {
    pub base_url: String,
    pub credential: VertexAiCredential,
}

// Not Debug: both variants are secrets.
#[derive(Clone)]
pub enum VertexAiCredential {
    ApiKey(String),
    /// Service account key JSON; the transport exchanges it for an access token.
    ServiceAccount(String),
}

/// Without a project, Vertex AI Express serves the location chosen at signup.
fn base_url(region: &HostLabel, project_id: Option<&str>) -> Result<String, DomainError> {
    let Some(project_id) = project_id else {
        return Ok(format!("{VERTEX_AI_GLOBAL_HOST}/v1"));
    };
    let url = append_endpoint_segments(
        &host(region),
        &["v1", "projects", project_id, "locations", region.as_str()],
    )?;
    Ok(url.into())
}

fn parse_region(raw: &str) -> Result<HostLabel, DomainError> {
    let raw = raw.trim();
    let region = if raw.is_empty() {
        VERTEX_AI_DEFAULT_REGION.to_string()
    } else {
        raw.to_ascii_lowercase()
    };
    HostLabel::parse("Vertex AI region", &region)
}

fn host(region: &HostLabel) -> String {
    match region.as_str() {
        "global" => VERTEX_AI_GLOBAL_HOST.to_string(),
        "us" | "eu" => format!("https://aiplatform.{region}.rep.googleapis.com"),
        _ => format!("https://{region}-aiplatform.googleapis.com"),
    }
}

fn service_account_project_id(service_account_json: &str) -> Result<String, DomainError> {
    #[derive(Deserialize)]
    struct ServiceAccount {
        project_id: Option<String>,
    }

    let account: ServiceAccount = serde_json::from_str(service_account_json).map_err(|error| {
        DomainError::InvalidData(format!(
            "Vertex AI service account JSON parse failed: {error}"
        ))
    })?;
    account
        .project_id
        .map(|project_id| project_id.trim().to_string())
        .filter(|project_id| !project_id.is_empty())
        .ok_or_else(|| {
            DomainError::InvalidData(
                "Vertex AI service account JSON is missing project_id".to_string(),
            )
        })
}
