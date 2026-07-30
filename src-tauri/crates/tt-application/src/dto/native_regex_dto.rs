use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeRegexBatchRequestDto {
    #[serde(default)]
    pub tasks: Vec<NativeRegexTaskDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeRegexTaskDto {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub scripts: Vec<NativeRegexScriptDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeRegexScriptDto {
    #[serde(default)]
    pub script_name: String,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub flags: String,
    #[serde(default)]
    pub global: bool,
    #[serde(default)]
    pub replacement: String,
    #[serde(default)]
    pub trim_strings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeRegexBatchResponseDto {
    pub tasks: Vec<NativeRegexTaskResultDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NativeRegexTaskResultDto {
    pub text: String,
}
