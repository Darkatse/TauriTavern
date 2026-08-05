use std::{
    collections::{BTreeMap, HashSet, btree_map::Entry},
    fmt,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;

use crate::errors::DomainError;

const TOOL_ID_SEPARATOR: char = ':';
const BUILTIN_TOOL_PROVIDER_ID: &str = "builtin";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolProviderId(String);

impl ToolProviderId {
    pub fn parse(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(DomainError::InvalidData(
                "tool.provider_id_empty: tool provider id cannot be empty".to_string(),
            ));
        }
        if raw.contains(TOOL_ID_SEPARATOR) {
            return Err(DomainError::InvalidData(format!(
                "tool.provider_id_invalid: tool provider id `{raw}` cannot contain `{TOOL_ID_SEPARATOR}`"
            )));
        }

        Ok(Self(raw))
    }

    pub fn builtin() -> Self {
        Self(BUILTIN_TOOL_PROVIDER_ID.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ToolProviderId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ToolProviderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolId(String);

impl ToolId {
    pub fn new(
        provider_id: &ToolProviderId,
        native_name: impl AsRef<str>,
    ) -> Result<Self, DomainError> {
        let native_name = native_name.as_ref();
        if native_name.is_empty() {
            return Err(DomainError::InvalidData(
                "tool.native_name_empty: tool native name cannot be empty".to_string(),
            ));
        }

        Ok(Self(format!(
            "{}{TOOL_ID_SEPARATOR}{native_name}",
            provider_id.as_str()
        )))
    }

    pub fn parse(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        let (provider_id, native_name) = raw.split_once(TOOL_ID_SEPARATOR).ok_or_else(|| {
            DomainError::InvalidData(format!(
                "tool.id_invalid: tool id `{raw}` must contain a provider and native name"
            ))
        })?;
        let provider_id = ToolProviderId::parse(provider_id.to_string())?;
        Self::new(&provider_id, native_name)
    }

    pub fn builtin(native_name: impl AsRef<str>) -> Result<Self, DomainError> {
        Self::new(&ToolProviderId::builtin(), native_name)
    }

    pub fn provider_id(&self) -> &str {
        self.0
            .split_once(TOOL_ID_SEPARATOR)
            .expect("ToolId constructor guarantees a separator")
            .0
    }

    pub fn native_name(&self) -> &str {
        self.0
            .split_once(TOOL_ID_SEPARATOR)
            .expect("ToolId constructor guarantees a separator")
            .1
    }

    pub fn is_builtin(&self) -> bool {
        self.provider_id() == BUILTIN_TOOL_PROVIDER_ID
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ToolId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ToolId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub id: ToolId,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub annotations: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct ToolSnapshotId(String);

impl ToolSnapshotId {
    pub fn parse(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(DomainError::InvalidData(
                "tool.snapshot_id_empty: tool snapshot id cannot be empty".to_string(),
            ));
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolSnapshotId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolBinding {
    descriptor: ToolDescriptor,
    model_alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_calls: Option<usize>,
}

impl ToolBinding {
    pub fn new(
        descriptor: ToolDescriptor,
        model_alias: impl Into<String>,
        max_calls: Option<usize>,
    ) -> Result<Self, DomainError> {
        let model_alias = model_alias.into();
        if model_alias.is_empty() {
            return Err(DomainError::InvalidData(format!(
                "tool.snapshot_alias_empty: tool `{}` has an empty model alias",
                descriptor.id
            )));
        }
        if max_calls == Some(0) {
            return Err(DomainError::InvalidData(format!(
                "tool.snapshot_tool_budget_invalid: tool `{}` max calls must be greater than zero",
                descriptor.id
            )));
        }
        Ok(Self {
            descriptor,
            model_alias,
            max_calls,
        })
    }

    pub fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    pub fn tool_id(&self) -> &ToolId {
        &self.descriptor.id
    }

    pub fn model_alias(&self) -> &str {
        &self.model_alias
    }

    pub fn max_calls(&self) -> Option<usize> {
        self.max_calls
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvocationToolSnapshot {
    schema_version: u32,
    id: ToolSnapshotId,
    bindings: Vec<ToolBinding>,
    max_calls_per_invocation: usize,
}

impl InvocationToolSnapshot {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn try_new(
        id: ToolSnapshotId,
        bindings: Vec<ToolBinding>,
        max_calls_per_invocation: usize,
    ) -> Result<Self, DomainError> {
        if max_calls_per_invocation == 0 {
            return Err(DomainError::InvalidData(
                "tool.snapshot_budget_invalid: max calls per invocation must be greater than zero"
                    .to_string(),
            ));
        }

        let mut tool_ids = HashSet::with_capacity(bindings.len());
        let mut aliases = HashSet::with_capacity(bindings.len());
        for binding in &bindings {
            if !tool_ids.insert(binding.tool_id().clone()) {
                return Err(DomainError::Conflict(format!(
                    "tool.snapshot_duplicate_id: duplicate tool id `{}`",
                    binding.tool_id()
                )));
            }
            if !aliases.insert(binding.model_alias().to_string()) {
                return Err(DomainError::Conflict(format!(
                    "tool.snapshot_duplicate_alias: duplicate model alias `{}`",
                    binding.model_alias()
                )));
            }
        }

        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            id,
            bindings,
            max_calls_per_invocation,
        })
    }

    pub fn id(&self) -> &ToolSnapshotId {
        &self.id
    }

    pub fn bindings(&self) -> &[ToolBinding] {
        &self.bindings
    }

    pub fn max_calls_per_invocation(&self) -> usize {
        self.max_calls_per_invocation
    }

    pub fn binding(&self, tool_id: &ToolId) -> Option<&ToolBinding> {
        // ponytail: invocation tool surfaces are small; add a derived index only if profiling
        // shows linear lookup matters.
        self.bindings
            .iter()
            .find(|binding| binding.tool_id() == tool_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCatalog {
    descriptors: BTreeMap<ToolId, ToolDescriptor>,
}

impl ToolCatalog {
    pub fn try_from_descriptors(
        descriptors: impl IntoIterator<Item = ToolDescriptor>,
    ) -> Result<Self, DomainError> {
        let mut catalog = BTreeMap::new();
        for descriptor in descriptors {
            match catalog.entry(descriptor.id.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(descriptor);
                }
                Entry::Occupied(entry) => {
                    return Err(DomainError::Conflict(format!(
                        "tool.catalog_duplicate_id: duplicate tool id `{}`",
                        entry.key()
                    )));
                }
            }
        }

        Ok(Self {
            descriptors: catalog,
        })
    }

    pub fn get(&self, id: &ToolId) -> Option<&ToolDescriptor> {
        self.descriptors.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ToolDescriptor> {
        self.descriptors.values()
    }

    pub fn len(&self) -> usize {
        self.descriptors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    None,
    Auto,
    Required,
    Specific(ToolId),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocation {
    pub call_id: String,
    pub tool_id: ToolId,
    pub arguments: Value,
    #[serde(default)]
    pub provider_metadata: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolTurnContract {
    snapshot_id: ToolSnapshotId,
    tools: Vec<ToolId>,
    choice: ToolChoice,
}

impl ToolTurnContract {
    pub fn all(snapshot: &InvocationToolSnapshot, choice: ToolChoice) -> Result<Self, DomainError> {
        let tools = snapshot
            .bindings()
            .iter()
            .map(|binding| binding.tool_id().clone())
            .collect::<Vec<_>>();

        if matches!(choice, ToolChoice::Required) && tools.is_empty() {
            return Err(DomainError::InvalidData(
                "tool.turn_required_empty: required tool choice needs at least one tool"
                    .to_string(),
            ));
        }
        if let ToolChoice::Specific(tool_id) = &choice
            && snapshot.binding(tool_id).is_none()
        {
            return Err(DomainError::InvalidData(format!(
                "tool.turn_specific_not_available: specific tool `{tool_id}` is not available in snapshot `{}`",
                snapshot.id()
            )));
        }

        Ok(Self {
            snapshot_id: snapshot.id().clone(),
            tools,
            choice,
        })
    }

    pub fn snapshot_id(&self) -> &ToolSnapshotId {
        &self.snapshot_id
    }

    pub fn tools(&self) -> &[ToolId] {
        &self.tools
    }

    pub fn choice(&self) -> &ToolChoice {
        &self.choice
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        InvocationToolSnapshot, ToolBinding, ToolCatalog, ToolChoice, ToolDescriptor, ToolId,
        ToolProviderId, ToolSnapshotId, ToolTurnContract,
    };
    use crate::errors::DomainError;

    fn descriptor(id: ToolId) -> ToolDescriptor {
        ToolDescriptor {
            id,
            title: None,
            description: None,
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            annotations: json!({}),
        }
    }

    #[test]
    fn tool_identity_is_stable_and_opaque() {
        let builtin = ToolProviderId::builtin();
        let mcp = ToolProviderId::parse("mcp/registration-1").unwrap();
        let builtin_id = ToolId::new(&builtin, "workspace.read_file").unwrap();
        let mcp_id = ToolId::new(&mcp, "workspace.read_file").unwrap();

        assert_eq!(builtin_id.as_str(), "builtin:workspace.read_file");
        assert_eq!(builtin_id.provider_id(), "builtin");
        assert_eq!(builtin_id.native_name(), "workspace.read_file");
        assert_ne!(builtin_id, mcp_id);
    }

    #[test]
    fn tool_identity_rejects_invalid_serialized_values() {
        for invalid in ["", "builtin", ":tool", "builtin:"] {
            assert!(serde_json::from_value::<ToolId>(json!(invalid)).is_err());
        }
        assert!(ToolProviderId::parse("mcp:registration-1").is_err());
    }

    #[test]
    fn tool_choice_uses_canonical_domain_shape() {
        let choice = ToolChoice::Specific(ToolId::builtin("workspace.finish").unwrap());
        let value = serde_json::to_value(&choice).unwrap();

        assert_eq!(value, json!({ "specific": "builtin:workspace.finish" }));
        assert_eq!(serde_json::from_value::<ToolChoice>(value).unwrap(), choice);
    }

    #[test]
    fn snapshot_preserves_binding_order_and_rejects_duplicate_aliases() {
        let first = ToolBinding::new(
            descriptor(ToolId::builtin("first").unwrap()),
            "first_alias",
            Some(2),
        )
        .unwrap();
        let second = ToolBinding::new(
            descriptor(ToolId::builtin("second").unwrap()),
            "second_alias",
            None,
        )
        .unwrap();
        let snapshot = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("inv_root").unwrap(),
            vec![second.clone(), first.clone()],
            4,
        )
        .unwrap();

        assert_eq!(snapshot.bindings(), &[second, first.clone()]);
        assert_eq!(snapshot.max_calls_per_invocation(), 4);
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap()["bindings"][0]["modelAlias"],
            "second_alias"
        );

        let duplicate = ToolBinding::new(
            descriptor(ToolId::builtin("third").unwrap()),
            "second_alias",
            None,
        )
        .unwrap();
        let error = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("duplicate").unwrap(),
            vec![snapshot.bindings()[0].clone(), duplicate],
            4,
        )
        .unwrap_err();
        assert!(
            matches!(error, DomainError::Conflict(message) if message.contains("tool.snapshot_duplicate_alias"))
        );

        let error = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("duplicate-id").unwrap(),
            vec![first.clone(), first],
            4,
        )
        .unwrap_err();
        assert!(
            matches!(error, DomainError::Conflict(message) if message.contains("tool.snapshot_duplicate_id"))
        );
    }

    #[test]
    fn turn_uses_the_complete_snapshot_and_validates_choice() {
        let first_id = ToolId::builtin("first").unwrap();
        let second_id = ToolId::builtin("second").unwrap();
        let snapshot = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("inv_root").unwrap(),
            vec![
                ToolBinding::new(descriptor(first_id.clone()), "first", None).unwrap(),
                ToolBinding::new(descriptor(second_id.clone()), "second", None).unwrap(),
            ],
            4,
        )
        .unwrap();

        let expected = vec![first_id, second_id.clone()];
        let turn = ToolTurnContract::all(&snapshot, ToolChoice::Specific(second_id)).unwrap();
        assert_eq!(turn.tools(), expected.as_slice());

        let unknown = ToolId::builtin("unknown").unwrap();
        assert!(ToolTurnContract::all(&snapshot, ToolChoice::Specific(unknown)).is_err());

        let empty =
            InvocationToolSnapshot::try_new(ToolSnapshotId::parse("empty").unwrap(), Vec::new(), 4)
                .unwrap();
        assert!(ToolTurnContract::all(&empty, ToolChoice::Required).is_err());
    }

    #[test]
    fn tool_catalog_orders_by_id_and_keeps_provider_namespaces_distinct() {
        let builtin_id = ToolId::builtin("search").unwrap();
        let mcp_id = ToolId::new(
            &ToolProviderId::parse("mcp/registration-1").unwrap(),
            "search",
        )
        .unwrap();
        let catalog = ToolCatalog::try_from_descriptors([
            descriptor(mcp_id.clone()),
            descriptor(builtin_id.clone()),
        ])
        .unwrap();

        assert_eq!(catalog.len(), 2);
        assert!(!catalog.is_empty());
        assert_eq!(catalog.get(&builtin_id).unwrap().id, builtin_id);
        assert_eq!(
            catalog
                .iter()
                .map(|descriptor| descriptor.id.as_str())
                .collect::<Vec<_>>(),
            ["builtin:search", "mcp/registration-1:search"]
        );
    }

    #[test]
    fn tool_catalog_rejects_duplicate_ids() {
        let id = ToolId::builtin("workspace.finish").unwrap();
        let error =
            ToolCatalog::try_from_descriptors([descriptor(id.clone()), descriptor(id.clone())])
                .unwrap_err();

        assert!(matches!(
            error,
            DomainError::Conflict(message)
                if message
                    == "tool.catalog_duplicate_id: duplicate tool id `builtin:workspace.finish`"
        ));
    }

    #[test]
    fn tool_catalog_can_be_empty() {
        let catalog = ToolCatalog::try_from_descriptors(Vec::new()).unwrap();

        assert!(catalog.is_empty());
        assert_eq!(catalog.iter().count(), 0);
    }
}
