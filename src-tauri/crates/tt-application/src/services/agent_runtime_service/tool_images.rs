#[derive(Debug, Clone)]
pub(crate) struct IncomingToolImage {
    pub data: Vec<u8>,
}

use base64::Engine;
use serde_json::json;
use tt_domain::models::agent::WorkspacePath;
use tt_domain::models::agent::{AgentModelContentPart, AgentModelRequest};
use tt_ports::tool_images::MAX_TOOL_IMAGE_BYTES;

use super::AgentRuntimeService;
use crate::errors::ApplicationError;

impl AgentRuntimeService {
    pub(super) async fn hydrate_tool_images(
        &self,
        run_id: &str,
        request: &AgentModelRequest,
        accepts_images: bool,
    ) -> Result<AgentModelRequest, ApplicationError> {
        let mut request = request.clone();
        for message in &mut request.messages {
            let mut parts = Vec::new();
            for mut part in std::mem::take(&mut message.parts) {
                let mut media = Vec::new();
                if let AgentModelContentPart::ToolResult { result } = &mut part {
                    for image in &result.images {
                        if accepts_images {
                            let path = WorkspacePath::parse(&image.path)?;
                            let bytes = self
                                .workspace_files(run_id)
                                .await?
                                .read_file(&path, MAX_TOOL_IMAGE_BYTES)
                                .await?;
                            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                            media.push(AgentModelContentPart::Media {
                                mime_type: image.mime_type.clone(),
                                value: json!({ "type": "image_url", "image_url": { "url": format!("data:{};base64,{data}", image.mime_type) } }),
                            });
                        } else {
                            result.content.push_str(
                                "\n\n[The image is not shown: this model does not accept images.]",
                            );
                        }
                    }
                }
                parts.push(part);
                parts.extend(media);
            }
            message.parts = parts;
        }
        Ok(request)
    }
}
