use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tt_domain::{
    errors::DomainError,
    models::{
        agent::{AgentChatRef, AgentRunTarget},
        tool::{AgentToolScope, ToolDescriptor, ToolId},
    },
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolDefinition {
    pub extension_id: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub contexts: Vec<AgentToolScope>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl ExtensionToolDefinition {
    pub fn into_tool(self) -> Result<ExtensionTool, DomainError> {
        let id = ToolId::extension(&self.extension_id, &self.name)?;
        if self.description.trim().is_empty() || self.contexts.is_empty() {
            return Err(DomainError::InvalidData(
                "extension.tool_definition_invalid: description and contexts must not be empty"
                    .into(),
            ));
        }
        if !self.input_schema.is_object() || self.input_schema["type"] != "object" {
            return Err(DomainError::InvalidData(
                "extension.tool_schema_invalid: inputSchema must describe an object".into(),
            ));
        }
        Ok(ExtensionTool {
            descriptor: ToolDescriptor {
                id,
                title: Some(self.name),
                description: Some(self.description),
                input_schema: self.input_schema,
                output_schema: None,
                annotations: Value::Null,
            },
            contexts: self.contexts,
            enabled: self.enabled,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionTool {
    pub descriptor: ToolDescriptor,
    pub contexts: Vec<AgentToolScope>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExtensionToolTarget {
    Chat {
        stable_chat_id: String,
        chat_ref: AgentChatRef,
    },
    Session {
        session_id: String,
    },
}

impl From<&AgentRunTarget> for ExtensionToolTarget {
    fn from(target: &AgentRunTarget) -> Self {
        match target {
            AgentRunTarget::Chat(chat) => Self::Chat {
                stable_chat_id: chat.stable_chat_id.clone(),
                chat_ref: chat.chat_ref.clone(),
            },
            AgentRunTarget::Session { session_id } => Self::Session {
                session_id: session_id.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionToolCall {
    pub tool_id: ToolId,
    pub run_id: String,
    pub invocation_id: String,
    pub call_id: String,
    pub target: ExtensionToolTarget,
    pub arguments: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExtensionToolEvent {
    Call {
        request_id: String,
        call: ExtensionToolCall,
    },
    Cancel {
        request_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExtensionToolReply {
    Value { value: Value },
    Error { message: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExtensionToolEnabledDto {
    pub tool_id: ToolId,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveExtensionToolCallDto {
    pub request_id: String,
    pub result: ExtensionToolReply,
}
