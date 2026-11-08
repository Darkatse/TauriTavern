use thiserror::Error;
use tt_domain::models::tool::{
    InvocationToolSnapshot, ToolChoice, ToolId, ToolInvocation, ToolSnapshotId, ToolTurnContract,
};

pub(crate) fn authorize(
    snapshot: &InvocationToolSnapshot,
    turn: &ToolTurnContract,
    invocation: &ToolInvocation,
) -> Result<(), ToolRequestGateError> {
    if turn.snapshot_id() != snapshot.id() {
        return Err(ToolRequestGateError::TurnSnapshotMismatch {
            turn_snapshot_id: turn.snapshot_id().clone(),
            invocation_snapshot_id: snapshot.id().clone(),
        });
    }

    snapshot.binding(&invocation.tool_id).ok_or_else(|| {
        ToolRequestGateError::ToolNotInSnapshot {
            tool_id: invocation.tool_id.clone(),
            snapshot_id: snapshot.id().clone(),
        }
    })?;

    match turn.choice() {
        ToolChoice::None => {
            return Err(ToolRequestGateError::ToolChoiceNone {
                tool_id: invocation.tool_id.clone(),
            });
        }
        ToolChoice::Specific(required_tool_id) if required_tool_id != &invocation.tool_id => {
            return Err(ToolRequestGateError::ToolChoiceSpecific {
                tool_id: invocation.tool_id.clone(),
                required_tool_id: required_tool_id.clone(),
            });
        }
        ToolChoice::Auto | ToolChoice::Required | ToolChoice::Specific(_) => {}
    }

    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum ToolRequestGateError {
    #[error(
        "tool.turn_snapshot_mismatch: turn references snapshot `{turn_snapshot_id}` but invocation uses `{invocation_snapshot_id}`"
    )]
    TurnSnapshotMismatch {
        turn_snapshot_id: ToolSnapshotId,
        invocation_snapshot_id: ToolSnapshotId,
    },
    #[error(
        "model.unknown_tool_call: tool `{tool_id}` is not available in snapshot `{snapshot_id}`"
    )]
    ToolNotInSnapshot {
        tool_id: ToolId,
        snapshot_id: ToolSnapshotId,
    },
    #[error(
        "model.tool_choice_violation: tool `{tool_id}` is forbidden by the current tool choice"
    )]
    ToolChoiceNone { tool_id: ToolId },
    #[error(
        "model.tool_choice_violation: current tool choice requires `{required_tool_id}`, not `{tool_id}`"
    )]
    ToolChoiceSpecific {
        tool_id: ToolId,
        required_tool_id: ToolId,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tt_domain::models::tool::{
        InvocationToolSnapshot, ToolArguments, ToolBinding, ToolChoice, ToolDescriptor, ToolId,
        ToolInvocation, ToolSnapshotId, ToolTurnContract,
    };

    use super::{ToolRequestGateError, authorize};

    fn binding(tool_id: ToolId) -> ToolBinding {
        let model_alias = tool_id.as_str().to_string();
        ToolBinding::new(
            ToolDescriptor {
                id: tool_id.clone(),
                title: None,
                description: None,
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                annotations: Value::Null,
            },
            model_alias,
        )
        .unwrap()
    }

    fn invocation(call_id: &str, tool_id: ToolId) -> ToolInvocation {
        ToolInvocation {
            call_id: call_id.to_string(),
            tool_id,
            arguments: ToolArguments::empty(),
            provider_metadata: Value::Null,
        }
    }

    #[test]
    fn gate_enforces_snapshot_and_turn_choice() {
        let first = ToolId::builtin("first").unwrap();
        let second = ToolId::builtin("second").unwrap();
        let snapshot = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("invocation").unwrap(),
            vec![binding(first.clone()), binding(second.clone())],
        )
        .unwrap();
        let forbidden_turn = ToolTurnContract::all(&snapshot, ToolChoice::None).unwrap();
        let allowed_turn = ToolTurnContract::all(&snapshot, ToolChoice::Auto).unwrap();
        let specific_turn =
            ToolTurnContract::all(&snapshot, ToolChoice::Specific(first.clone())).unwrap();
        let other_snapshot = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("other-invocation").unwrap(),
            vec![binding(first.clone())],
        )
        .unwrap();
        let other_turn = ToolTurnContract::all(&other_snapshot, ToolChoice::Auto).unwrap();

        assert!(matches!(
            authorize(
                &snapshot,
                &other_turn,
                &invocation("snapshot", first.clone())
            ),
            Err(ToolRequestGateError::TurnSnapshotMismatch { .. })
        ));
        assert!(matches!(
            authorize(
                &snapshot,
                &forbidden_turn,
                &invocation("none", first.clone())
            ),
            Err(ToolRequestGateError::ToolChoiceNone { .. })
        ));
        assert!(matches!(
            authorize(&snapshot, &specific_turn, &invocation("specific", second)),
            Err(ToolRequestGateError::ToolChoiceSpecific { .. })
        ));
        assert!(matches!(
            authorize(
                &snapshot,
                &allowed_turn,
                &invocation("unknown", ToolId::builtin("unknown").unwrap())
            ),
            Err(ToolRequestGateError::ToolNotInSnapshot { .. })
        ));
        authorize(
            &snapshot,
            &allowed_turn,
            &invocation("allowed", first.clone()),
        )
        .unwrap();
    }
}
