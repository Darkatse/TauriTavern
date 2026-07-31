use tt_domain::errors::DomainError;
use tt_domain::models::agent::AgentToolSpec;
use tt_domain::models::tool::{ToolCatalog, ToolDescriptor, ToolId};

pub(super) fn project_builtin_catalog(specs: &[AgentToolSpec]) -> Result<ToolCatalog, DomainError> {
    let descriptors = specs
        .iter()
        .map(project_builtin_descriptor)
        .collect::<Result<Vec<_>, _>>()?;

    ToolCatalog::try_from_descriptors(descriptors)
}

fn project_builtin_descriptor(spec: &AgentToolSpec) -> Result<ToolDescriptor, DomainError> {
    Ok(ToolDescriptor {
        id: ToolId::builtin(&spec.name)?,
        title: Some(spec.title.clone()),
        description: Some(spec.description.clone()),
        input_schema: spec.input_schema.clone(),
        output_schema: spec.output_schema.clone(),
        annotations: spec.annotations.clone(),
    })
}
