use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearxngSearchRequestDto {
    pub base_url: String,
    pub query: String,
    #[serde(default)]
    pub preferences: Option<String>,
    #[serde(default)]
    pub categories: Option<String>,
}
