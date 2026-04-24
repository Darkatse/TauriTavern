use std::ffi::OsStr;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::fs;
use tokio::sync::watch;
use tokio::time::{Duration, sleep};

use crate::domain::errors::DomainError;
use crate::domain::models::character::sanitize_filename;
use crate::domain::repositories::stable_diffusion_repository::{
    SdRouteRequest, SdRouteResponse, SdRouteResponseKind, StableDiffusionRepository,
};
use crate::infrastructure::http_client_pool::{HttpClientPool, HttpClientProfile};
use crate::infrastructure::sync_fs;

pub struct HttpStableDiffusionRepository {
    http_clients: Arc<HttpClientPool>,
    comfy_workflows_dir: PathBuf,
}

impl HttpStableDiffusionRepository {
    pub fn new(http_clients: Arc<HttpClientPool>, comfy_workflows_dir: PathBuf) -> Self {
        Self {
            http_clients,
            comfy_workflows_dir,
        }
    }
}

#[async_trait]
impl StableDiffusionRepository for HttpStableDiffusionRepository {
    async fn handle(
        &self,
        request: SdRouteRequest,
        cancel: watch::Receiver<bool>,
    ) -> Result<SdRouteResponse, DomainError> {
        let path = request.path.trim().trim_start_matches('/').to_string();

        match path.as_str() {
            // WebUI / SD.Next (local chain)
            "ping" => webui_ping(&self.http_clients, &request.body).await,
            "upscalers" => webui_upscalers(&self.http_clients, &request.body).await,
            "sd-next/upscalers" => webui_sdnext_upscalers(&self.http_clients, &request.body).await,
            "vaes" => webui_vaes(&self.http_clients, &request.body).await,
            "samplers" => webui_samplers(&self.http_clients, &request.body).await,
            "schedulers" => webui_schedulers(&self.http_clients, &request.body).await,
            "models" => webui_models(&self.http_clients, &request.body).await,
            "get-model" => webui_get_model(&self.http_clients, &request.body).await,
            "set-model" => webui_set_model(&self.http_clients, &request.body, cancel).await,
            "generate" => webui_generate(&self.http_clients, request.body, cancel).await,

            // ComfyUI (local chain)
            "comfy/ping" => comfy_ping(&self.http_clients, &request.body).await,
            "comfy/samplers" => comfy_samplers(&self.http_clients, &request.body).await,
            "comfy/models" => comfy_models(&self.http_clients, &request.body).await,
            "comfy/schedulers" => comfy_schedulers(&self.http_clients, &request.body).await,
            "comfy/vaes" => comfy_vaes(&self.http_clients, &request.body).await,
            "comfy/generate" => comfy_generate(&self.http_clients, &request.body, cancel).await,

            // Comfy workflows (local files)
            "comfy/workflows" => comfy_list_workflows(&self.comfy_workflows_dir).await,
            "comfy/workflow" => comfy_read_workflow(&self.comfy_workflows_dir, &request.body).await,
            "comfy/save-workflow" => {
                comfy_save_workflow(&self.comfy_workflows_dir, &request.body).await
            }
            "comfy/delete-workflow" => {
                comfy_delete_workflow(&self.comfy_workflows_dir, &request.body).await
            }
            "comfy/rename-workflow" => {
                comfy_rename_workflow(&self.comfy_workflows_dir, &request.body).await
            }

            // stable-diffusion.cpp (local chain)
            "sdcpp/ping" => sdcpp_ping(&self.http_clients, &request.body).await,
            "sdcpp/generate" => sdcpp_generate(&self.http_clients, &request.body, cancel).await,

            // DrawThings (local chain)
            "drawthings/ping" => drawthings_ping(&self.http_clients, &request.body).await,
            "drawthings/get-model" => {
                drawthings_get_field(&self.http_clients, &request.body, "model").await
            }
            "drawthings/get-upscaler" => {
                drawthings_get_field(&self.http_clients, &request.body, "upscaler").await
            }
            "drawthings/generate" => {
                drawthings_generate(&self.http_clients, &request.body, cancel).await
            }

            // Cloud endpoints intentionally not implemented in this build.
            _ => Ok(text(
                501,
                "Cloud provider endpoints are not implemented in this build.",
            )),
        }
    }
}

fn json_response(status: u16, body: Value) -> SdRouteResponse {
    SdRouteResponse {
        status,
        kind: SdRouteResponseKind::Json,
        body,
    }
}

fn text(status: u16, message: impl Into<String>) -> SdRouteResponse {
    SdRouteResponse {
        status,
        kind: SdRouteResponseKind::Text,
        body: Value::String(message.into()),
    }
}

fn empty(status: u16) -> SdRouteResponse {
    SdRouteResponse {
        status,
        kind: SdRouteResponseKind::Empty,
        body: Value::Null,
    }
}

fn http_client(http_clients: &Arc<HttpClientPool>) -> Result<reqwest::Client, DomainError> {
    http_clients.client(HttpClientProfile::ImageGeneration)
}

fn require_string(body: &Value, key: &str) -> Result<String, DomainError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DomainError::InvalidData(format!("Missing required field: {}", key)))
}

fn optional_string(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn parse_url(raw: &str) -> Result<Url, DomainError> {
    Url::parse(raw.trim())
        .map_err(|error| DomainError::InvalidData(format!("Invalid url: {error}")))
}

fn basic_auth_header(auth: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(auth);
    format!("Basic {encoded}")
}

fn unset_override_settings_forge_additional_modules(body: &mut Value) {
    let Some(override_settings) = body.get_mut("override_settings") else {
        return;
    };
    let Some(map) = override_settings.as_object_mut() else {
        return;
    };
    map.remove("forge_additional_modules");
}

fn ensure_json_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn read_workflow_names(dir: &Path) -> Result<Vec<String>, DomainError> {
    let mut entries = fs::read_dir(dir)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let mut names = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };

        if file_name.starts_with('.') {
            continue;
        }

        if !ensure_json_extension(file_name) {
            continue;
        }

        names.push(file_name.to_string());
    }

    names.sort();
    Ok(names)
}

#[derive(Debug, Deserialize)]
struct NamedItem {
    name: String,
}

#[derive(Debug, Deserialize)]
struct TitleItem {
    title: String,
}

#[derive(Debug, Deserialize)]
struct ProgressInner {
    job_count: u64,
}

#[derive(Debug, Deserialize)]
struct ProgressState {
    progress: f64,
    state: ProgressInner,
}

async fn webui_ping(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let mut options_url = parse_url(&url)?;
    options_url.set_path("/sdapi/v1/options");

    let client = http_client(http_clients)?;
    let response = client
        .get(options_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "SD WebUI returned an error.".to_string(),
        ));
    }

    Ok(empty(200))
}

async fn webui_upscalers(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let client = http_client(http_clients)?;

    let mut upscalers_url = parse_url(&url)?;
    upscalers_url.set_path("/sdapi/v1/upscalers");
    let mut latent_url = parse_url(&url)?;
    latent_url.set_path("/sdapi/v1/latent-upscale-modes");

    let upscalers_fut = client
        .get(upscalers_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send();

    let latent_fut = client
        .get(latent_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send();

    let (upscalers_res, latent_res) = tokio::try_join!(upscalers_fut, latent_fut)
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !upscalers_res.status().is_success() || !latent_res.status().is_success() {
        return Err(DomainError::InternalError(
            "SD WebUI returned an error.".to_string(),
        ));
    }

    let upscalers = upscalers_res
        .json::<Vec<NamedItem>>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    let latent = latent_res
        .json::<Vec<NamedItem>>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    let mut merged = upscalers;
    let insert_at = merged.len().min(1);
    merged.splice(insert_at..insert_at, latent);

    Ok(json_response(200, json!(merged)))
}

async fn webui_sdnext_upscalers(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let client = http_client(http_clients)?;

    let mut upscalers_url = parse_url(&url)?;
    upscalers_url.set_path("/sdapi/v1/upscalers");

    let response = client
        .get(upscalers_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "SD.Next returned an error.".to_string(),
        ));
    }

    let mut names = response
        .json::<Vec<NamedItem>>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    // Vlad doesn't provide latent upscalers through the API (upstream hardcodes them).
    let latent = vec![
        "Latent",
        "Latent (antialiased)",
        "Latent (bicubic)",
        "Latent (bicubic antialiased)",
        "Latent (nearest)",
        "Latent (nearest-exact)",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();

    let insert_at = names.len().min(1);
    names.splice(insert_at..insert_at, latent);

    Ok(json_response(200, json!(names)))
}

async fn webui_vaes(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let client = http_client(http_clients)?;

    let mut auto_url = parse_url(&url)?;
    auto_url.set_path("/sdapi/v1/sd-vae");
    let mut forge_url = parse_url(&url)?;
    forge_url.set_path("/sdapi/v1/sd-modules");

    let request = |target: Url| {
        client
            .get(target)
            .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
            .send()
    };

    let results = futures_util::future::join_all([request(auto_url), request(forge_url)]).await;

    for result in results {
        let response = match result {
            Ok(response) if response.status().is_success() => response,
            _ => continue,
        };

        let value = response
            .json::<Value>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let Some(array) = value.as_array() else {
            continue;
        };

        let names = array
            .iter()
            .filter_map(|item| item.get("model_name").and_then(Value::as_str))
            .map(|value| value.to_string())
            .collect::<Vec<_>>();

        return Ok(json_response(200, json!(names)));
    }

    Err(DomainError::InternalError(
        "SD WebUI returned an error.".to_string(),
    ))
}

async fn webui_samplers(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let client = http_client(http_clients)?;

    let mut samplers_url = parse_url(&url)?;
    samplers_url.set_path("/sdapi/v1/samplers");

    let response = client
        .get(samplers_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "SD WebUI returned an error.".to_string(),
        ));
    }

    let names = response
        .json::<Vec<NamedItem>>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    Ok(json_response(200, json!(names)))
}

async fn webui_schedulers(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let client = http_client(http_clients)?;

    let mut schedulers_url = parse_url(&url)?;
    schedulers_url.set_path("/sdapi/v1/schedulers");

    let response = client
        .get(schedulers_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "SD WebUI returned an error.".to_string(),
        ));
    }

    let names = response
        .json::<Vec<NamedItem>>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>();

    Ok(json_response(200, json!(names)))
}

async fn webui_models(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let client = http_client(http_clients)?;

    let mut models_url = parse_url(&url)?;
    models_url.set_path("/sdapi/v1/sd-models");

    let response = client
        .get(models_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "SD WebUI returned an error.".to_string(),
        ));
    }

    let models = response
        .json::<Vec<TitleItem>>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?
        .into_iter()
        .map(|item| {
            let title = item.title;
            json!({ "value": &title, "text": &title })
        })
        .collect::<Vec<_>>();

    Ok(json_response(200, json!(models)))
}

async fn webui_get_model(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let client = http_client(http_clients)?;

    let mut options_url = parse_url(&url)?;
    options_url.set_path("/sdapi/v1/options");

    let response = client
        .get(options_url)
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let value = response
        .json::<Value>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let name = value
        .get("sd_model_checkpoint")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    Ok(text(200, name))
}

async fn webui_set_model(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
    mut cancel: watch::Receiver<bool>,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");
    let model = require_string(body, "model")?;
    let client = http_client(http_clients)?;

    let mut options_url = parse_url(&url)?;
    options_url.set_path("/sdapi/v1/options");

    let response = client
        .post(options_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .json(&json!({ "sd_model_checkpoint": model }))
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "SD WebUI returned an error.".to_string(),
        ));
    }

    let mut progress_url = parse_url(&url)?;
    progress_url.set_path("/sdapi/v1/progress");

    const MAX_ATTEMPTS: usize = 10;
    const CHECK_INTERVAL: Duration = Duration::from_millis(2000);

    for _ in 0..MAX_ATTEMPTS {
        if *cancel.borrow() {
            return Err(DomainError::generation_cancelled_by_user());
        }

        let progress_fut = client
            .get(progress_url.clone())
            .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
            .send();

        let response = tokio::select! {
            res = progress_fut => res.map_err(|error| DomainError::InternalError(error.to_string()))?,
            changed = cancel.changed() => {
                let _ = changed;
                return Err(DomainError::generation_cancelled_by_user());
            }
        };

        let progress = response
            .json::<ProgressState>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        if progress.progress == 0.0 && progress.state.job_count == 0 {
            break;
        }

        tokio::select! {
            _ = sleep(CHECK_INTERVAL) => {},
            changed = cancel.changed() => {
                let _ = changed;
                return Err(DomainError::generation_cancelled_by_user());
            }
        }
    }

    Ok(empty(200))
}

async fn webui_generate(
    http_clients: &Arc<HttpClientPool>,
    mut body: Value,
    mut cancel: watch::Receiver<bool>,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(&body, "url")?;
    let auth = optional_string(&body, "auth");
    let client = http_client(http_clients)?;

    // Forge compatibility: try to remove forge_additional_modules if remote is not Forge.
    if let Ok(mut options_url) = parse_url(&url) {
        options_url.set_path("/sdapi/v1/options");
        let options_result = client
            .get(options_url)
            .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
            .send()
            .await;

        if let Ok(response) = options_result {
            if response.status().is_success() {
                if let Ok(value) = response.json::<Value>().await {
                    let is_forge = value.get("forge_preset").is_some();
                    if !is_forge {
                        unset_override_settings_forge_additional_modules(&mut body);
                    }
                }
            }
        }
    }

    let mut txt2img_url = parse_url(&url)?;
    txt2img_url.set_path("/sdapi/v1/txt2img");

    let request_fut = client
        .post(txt2img_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .json(&body)
        .send();

    let response = tokio::select! {
        res = request_fut => res.map_err(|error| DomainError::InternalError(error.to_string()))?,
        changed = cancel.changed() => {
            let _ = changed;

            if *cancel.borrow() {
                let mut interrupt_url = parse_url(&url)?;
                interrupt_url.set_path("/sdapi/v1/interrupt");
                let _ = client
                    .post(interrupt_url)
                    .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
                    .send()
                    .await;
                return Err(DomainError::generation_cancelled_by_user());
            }

            return Err(DomainError::generation_cancelled_by_user());
        }
    };

    if !response.status().is_success() {
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "SD WebUI returned an error.".to_string());
        return Err(DomainError::InternalError(format!(
            "SD WebUI returned an error: {}",
            text.trim()
        )));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    Ok(json_response(200, value))
}

async fn comfy_ping(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let base = parse_url(&url)?;
    let target = base
        .join("/system_stats")
        .map_err(|error| DomainError::InvalidData(format!("Invalid comfy url: {error}")))?;

    let client = http_client(http_clients)?;
    let response = client
        .get(target)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "ComfyUI returned an error.".to_string(),
        ));
    }

    Ok(empty(200))
}

async fn comfy_object_info(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<Value, DomainError> {
    let url = require_string(body, "url")?;
    let base = parse_url(&url)?;
    let target = base
        .join("/object_info")
        .map_err(|error| DomainError::InvalidData(format!("Invalid comfy url: {error}")))?;

    let client = http_client(http_clients)?;
    let response = client
        .get(target)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "ComfyUI returned an error.".to_string(),
        ));
    }

    response
        .json::<Value>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))
}

fn json_pointer<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value, DomainError> {
    value.pointer(pointer).ok_or_else(|| {
        DomainError::InternalError(format!("ComfyUI response missing field: {}", pointer))
    })
}

fn as_string_vec(value: &Value) -> Result<Vec<String>, DomainError> {
    let Some(array) = value.as_array() else {
        return Err(DomainError::InternalError("Expected array".to_string()));
    };

    Ok(array
        .iter()
        .filter_map(Value::as_str)
        .map(|value| value.to_string())
        .collect::<Vec<_>>())
}

async fn comfy_samplers(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let info = comfy_object_info(http_clients, body).await?;
    let value = json_pointer(&info, "/KSampler/input/required/sampler_name/0")?;
    Ok(json_response(200, json!(as_string_vec(value)?)))
}

async fn comfy_schedulers(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let info = comfy_object_info(http_clients, body).await?;
    let value = json_pointer(&info, "/KSampler/input/required/scheduler/0")?;
    Ok(json_response(200, json!(as_string_vec(value)?)))
}

async fn comfy_vaes(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let info = comfy_object_info(http_clients, body).await?;
    let value = json_pointer(&info, "/VAELoader/input/required/vae_name/0")?;
    Ok(json_response(200, json!(as_string_vec(value)?)))
}

async fn comfy_models(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let info = comfy_object_info(http_clients, body).await?;

    let ckpts = info
        .pointer("/CheckpointLoaderSimple/input/required/ckpt_name/0")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let unets = info
        .pointer("/UNETLoader/input/required/unet_name/0")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let ggufs = info
        .pointer("/UnetLoaderGGUF/input/required/unet_name/0")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut models = Vec::new();

    for item in ckpts {
        if let Some(name) = item.as_str() {
            models.push(json!({ "value": name, "text": name }));
        }
    }

    for item in unets {
        if let Some(name) = item.as_str() {
            models.push(json!({ "value": name, "text": format!("UNet: {}", name) }));
        }
    }

    for item in ggufs {
        if let Some(name) = item.as_str() {
            models.push(json!({ "value": name, "text": format!("GGUF: {}", name) }));
        }
    }

    for model in models.iter_mut() {
        let Some(text) = model
            .get("text")
            .and_then(Value::as_str)
            .map(|value| value.to_string())
        else {
            continue;
        };

        let pretty = text
            .rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(&text)
            .replace('_', " ");

        if let Some(map) = model.as_object_mut() {
            map.insert("text".to_string(), Value::String(pretty));
        }
    }

    Ok(json_response(200, json!(models)))
}

#[derive(Debug, Deserialize)]
struct ComfyPromptResponse {
    prompt_id: String,
}

async fn comfy_generate(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
    mut cancel: watch::Receiver<bool>,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let prompt = require_string(body, "prompt")?;
    let base = parse_url(&url)?;

    let prompt_url = base
        .join("/prompt")
        .map_err(|error| DomainError::InvalidData(format!("Invalid comfy url: {error}")))?;
    let history_url = base
        .join("/history")
        .map_err(|error| DomainError::InvalidData(format!("Invalid comfy url: {error}")))?;
    let interrupt_url = base
        .join("/interrupt")
        .map_err(|error| DomainError::InvalidData(format!("Invalid comfy url: {error}")))?;

    let client = http_client(http_clients)?;

    let prompt_request = client
        .post(prompt_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(prompt);

    let prompt_response = tokio::select! {
        res = prompt_request.send() => res.map_err(|error| DomainError::InternalError(error.to_string()))?,
        changed = cancel.changed() => {
            let _ = changed;
            let _ = client.post(interrupt_url).send().await;
            return Err(DomainError::generation_cancelled_by_user());
        }
    };

    if !prompt_response.status().is_success() {
        let text = prompt_response
            .text()
            .await
            .unwrap_or_else(|_| "ComfyUI returned an error.".to_string());
        return Err(DomainError::InternalError(format!(
            "ComfyUI returned an error: {}",
            text.trim()
        )));
    }

    let prompt_json = prompt_response
        .json::<ComfyPromptResponse>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let id = prompt_json.prompt_id;

    let item = loop {
        if *cancel.borrow() {
            let _ = client.post(interrupt_url.clone()).send().await;
            return Err(DomainError::generation_cancelled_by_user());
        }

        let history_request = client.get(history_url.clone());
        let history_response = tokio::select! {
            res = history_request.send() => res.map_err(|error| DomainError::InternalError(error.to_string()))?,
            changed = cancel.changed() => {
                let _ = changed;
                let _ = client.post(interrupt_url.clone()).send().await;
                return Err(DomainError::generation_cancelled_by_user());
            }
        };

        if !history_response.status().is_success() {
            return Err(DomainError::InternalError(
                "ComfyUI returned an error.".to_string(),
            ));
        }

        let history = history_response
            .json::<Value>()
            .await
            .map_err(|error| DomainError::InternalError(error.to_string()))?;

        let Some(entry) = history.get(&id) else {
            tokio::select! {
                _ = sleep(Duration::from_millis(100)) => {},
                changed = cancel.changed() => {
                    let _ = changed;
                    let _ = client.post(interrupt_url.clone()).send().await;
                    return Err(DomainError::generation_cancelled_by_user());
                }
            }
            continue;
        };

        break entry.clone();
    };

    // If ComfyUI reports an execution error, surface the traceback text like upstream.
    if item
        .pointer("/status/status_str")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "error")
    {
        let mut lines = Vec::new();

        if let Some(messages) = item.pointer("/status/messages").and_then(Value::as_array) {
            for message in messages {
                let Some(array) = message.as_array() else {
                    continue;
                };
                if array.len() < 2 {
                    continue;
                }
                if array[0].as_str() != Some("execution_error") {
                    continue;
                }

                let payload = &array[1];
                let node_type = payload
                    .get("node_type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let node_id = payload.get("node_id").and_then(Value::as_i64).unwrap_or(0);
                let exception_type = payload
                    .get("exception_type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let exception_message = payload
                    .get("exception_message")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                let line = format!(
                    "{} [{}] {}: {}",
                    node_type, node_id, exception_type, exception_message
                )
                .trim()
                .to_string();

                if !line.is_empty() {
                    lines.push(line);
                }
            }
        }

        let detail = if lines.is_empty() {
            "ComfyUI generation did not succeed.".to_string()
        } else {
            format!(
                "ComfyUI generation did not succeed.\n\n{}",
                lines.join("\n")
            )
        };

        return Err(DomainError::InternalError(detail));
    }

    let outputs = item
        .get("outputs")
        .and_then(Value::as_object)
        .ok_or_else(|| DomainError::InternalError("ComfyUI did not return outputs.".to_string()))?;

    let mut image_info = None;

    for output in outputs.values() {
        if let Some(images) = output.get("images").and_then(Value::as_array) {
            if let Some(first) = images.first() {
                image_info = Some(first.clone());
                break;
            }
        }
    }

    if image_info.is_none() {
        for output in outputs.values() {
            if let Some(gifs) = output.get("gifs").and_then(Value::as_array) {
                if let Some(first) = gifs.first() {
                    image_info = Some(first.clone());
                    break;
                }
            }
        }
    }

    let Some(info) = image_info else {
        return Err(DomainError::InternalError(
            "ComfyUI did not return any recognizable outputs.".to_string(),
        ));
    };

    let filename = info
        .get("filename")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DomainError::InternalError("ComfyUI output missing filename.".to_string())
        })?;
    let subfolder = info.get("subfolder").and_then(Value::as_str).unwrap_or("");
    let kind = info.get("type").and_then(Value::as_str).unwrap_or("output");

    let mut view_url = base
        .join("/view")
        .map_err(|error| DomainError::InvalidData(format!("Invalid comfy url: {error}")))?;
    view_url
        .query_pairs_mut()
        .append_pair("filename", filename)
        .append_pair("subfolder", subfolder)
        .append_pair("type", kind);

    let view_request = client.get(view_url);
    let view_response = tokio::select! {
        res = view_request.send() => res.map_err(|error| DomainError::InternalError(error.to_string()))?,
        changed = cancel.changed() => {
            let _ = changed;
            return Err(DomainError::generation_cancelled_by_user());
        }
    };

    if !view_response.status().is_success() {
        return Err(DomainError::InternalError(
            "ComfyUI returned an error.".to_string(),
        ));
    }

    let bytes = view_response
        .bytes()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;
    let format = Path::new(filename)
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("png")
        .to_lowercase();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);

    Ok(json_response(
        200,
        json!({ "format": format, "data": encoded }),
    ))
}

async fn comfy_list_workflows(dir: &Path) -> Result<SdRouteResponse, DomainError> {
    let names = read_workflow_names(dir).await?;
    Ok(json_response(200, json!(names)))
}

async fn comfy_read_workflow(dir: &Path, body: &Value) -> Result<SdRouteResponse, DomainError> {
    let raw = require_string(body, "file_name")?;
    let sanitized = sanitize_filename(&raw);

    if sanitized.is_empty() {
        return Err(DomainError::InvalidData(
            "Invalid workflow filename".to_string(),
        ));
    }

    let mut path = dir.join(&sanitized);
    if !path.exists() {
        path = dir.join("Default_Comfy_Workflow.json");
    }

    let content = fs::read_to_string(&path)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    Ok(json_response(200, Value::String(content)))
}

async fn comfy_save_workflow(dir: &Path, body: &Value) -> Result<SdRouteResponse, DomainError> {
    let raw = require_string(body, "file_name")?;
    let sanitized = sanitize_filename(&raw);

    if sanitized.is_empty() {
        return Err(DomainError::InvalidData(
            "Invalid workflow filename".to_string(),
        ));
    }

    let workflow = body
        .get("workflow")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let dest = dir.join(&sanitized);
    let mut reader = Cursor::new(workflow.into_bytes());
    sync_fs::write_file_atomic(&dest, &mut reader, now_ms()).await?;

    let names = read_workflow_names(dir).await?;
    Ok(json_response(200, json!(names)))
}

async fn comfy_delete_workflow(dir: &Path, body: &Value) -> Result<SdRouteResponse, DomainError> {
    let raw = require_string(body, "file_name")?;
    let sanitized = sanitize_filename(&raw);

    if sanitized.is_empty() {
        return Err(DomainError::InvalidData(
            "Invalid workflow filename".to_string(),
        ));
    }

    let path = dir.join(&sanitized);
    match fs::remove_file(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(DomainError::InternalError(error.to_string())),
    }

    Ok(empty(200))
}

async fn comfy_rename_workflow(dir: &Path, body: &Value) -> Result<SdRouteResponse, DomainError> {
    let old_raw = require_string(body, "old_name")?;
    let new_raw = require_string(body, "new_name")?;

    let old_sanitized = sanitize_filename(&old_raw);
    let new_sanitized = sanitize_filename(&new_raw);

    if old_sanitized.is_empty() || new_sanitized.is_empty() {
        return Ok(text(400, "Invalid workflow filename"));
    }

    if !ensure_json_extension(&old_sanitized) || !ensure_json_extension(&new_sanitized) {
        return Ok(text(400, "Only JSON workflow files are allowed"));
    }

    let old_path = dir.join(&old_sanitized);
    let new_path = dir.join(&new_sanitized);

    if !old_path.exists() {
        return Ok(text(404, "Workflow not found"));
    }

    if new_path.exists() {
        return Ok(text(409, "A workflow with that name already exists"));
    }

    fs::rename(&old_path, &new_path)
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    Ok(empty(204))
}

async fn sdcpp_ping(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let mut target = parse_url(&url)?;
    target.set_path("/v1/images/generations");

    let client = http_client(http_clients)?;
    let response = client
        .request(reqwest::Method::OPTIONS, target)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "stable-diffusion.cpp server returned an error.".to_string(),
        ));
    }

    Ok(empty(200))
}

fn maybe_insert(map: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    let Some(value) = value else {
        return;
    };

    if value.is_null() {
        return;
    }

    if value.as_str().is_some_and(|text| text.is_empty()) {
        return;
    }

    map.insert(key.to_string(), value.clone());
}

async fn sdcpp_generate(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
    mut cancel: watch::Receiver<bool>,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let base = parse_url(&url)?;
    let mut target = base.clone();
    target.set_path("/sdapi/v1/txt2img");

    let mut payload = Map::new();
    for key in [
        "prompt",
        "negative_prompt",
        "width",
        "height",
        "steps",
        "cfg_scale",
        "seed",
        "batch_size",
        "sampler_name",
        "scheduler",
        "clip_skip",
    ] {
        maybe_insert(&mut payload, key, body.get(key));
    }

    let client = http_client(http_clients)?;
    let request = client
        .post(target)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&Value::Object(payload));

    let response = tokio::select! {
        res = request.send() => res.map_err(|error| DomainError::InternalError(error.to_string()))?,
        changed = cancel.changed() => {
            let _ = changed;
            return Err(DomainError::generation_cancelled_by_user());
        }
    };

    if !response.status().is_success() {
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "stable-diffusion.cpp server returned an error.".to_string());
        return Err(DomainError::InternalError(format!(
            "stable-diffusion.cpp server returned an error: {}",
            text.trim()
        )));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    Ok(json_response(200, value))
}

async fn drawthings_ping(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let mut target = parse_url(&url)?;
    target.set_path("/");

    let client = http_client(http_clients)?;
    let response = client
        .head(target)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DomainError::InternalError(
            "SD DrawThings API returned an error.".to_string(),
        ));
    }

    Ok(empty(200))
}

async fn drawthings_get_field(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
    field: &str,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let mut target = parse_url(&url)?;
    target.set_path("/");

    let client = http_client(http_clients)?;
    let response = client
        .get(target)
        .send()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let value = response
        .json::<Value>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let field_value = value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(text(200, field_value))
}

async fn drawthings_generate(
    http_clients: &Arc<HttpClientPool>,
    body: &Value,
    mut cancel: watch::Receiver<bool>,
) -> Result<SdRouteResponse, DomainError> {
    let url = require_string(body, "url")?;
    let auth = optional_string(body, "auth");

    let mut target = parse_url(&url)?;
    target.set_path("/sdapi/v1/txt2img");

    let mut cloned = body.clone();
    if let Some(map) = cloned.as_object_mut() {
        map.remove("url");
        map.remove("auth");
    }

    let client = http_client(http_clients)?;
    let request = client
        .post(target)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::AUTHORIZATION, basic_auth_header(&auth))
        .json(&cloned);

    let response = tokio::select! {
        res = request.send() => res.map_err(|error| DomainError::InternalError(error.to_string()))?,
        changed = cancel.changed() => {
            let _ = changed;
            return Err(DomainError::generation_cancelled_by_user());
        }
    };

    if !response.status().is_success() {
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "SD DrawThings API returned an error.".to_string());
        return Err(DomainError::InternalError(format!(
            "SD DrawThings API returned an error: {}",
            text.trim()
        )));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    Ok(json_response(200, value))
}
