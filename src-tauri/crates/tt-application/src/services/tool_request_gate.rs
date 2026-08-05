use std::collections::HashMap;

use thiserror::Error;
use tt_domain::models::tool::{
    InvocationToolSnapshot, ToolChoice, ToolId, ToolInvocation, ToolSnapshotId, ToolTurnContract,
};

#[derive(Debug, Default)]
pub(crate) struct ToolRequestGate {
    total_calls: usize,
    calls_per_tool: HashMap<ToolId, usize>,
}

impl ToolRequestGate {
    pub(crate) fn authorize_and_reserve(
        &mut self,
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

        let binding = snapshot.binding(&invocation.tool_id).ok_or_else(|| {
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

        let max_calls = snapshot.max_calls_per_invocation();
        if self.total_calls >= max_calls {
            return Err(ToolRequestGateError::InvocationBudgetExhausted { max_calls });
        }

        let tool_calls = self
            .calls_per_tool
            .get(&invocation.tool_id)
            .copied()
            .unwrap_or(0);
        if let Some(max_calls) = binding.max_calls()
            && tool_calls >= max_calls
        {
            return Err(ToolRequestGateError::ToolBudgetExhausted {
                tool_id: invocation.tool_id.clone(),
                max_calls,
            });
        }

        self.total_calls += 1;
        *self
            .calls_per_tool
            .entry(invocation.tool_id.clone())
            .or_insert(0) += 1;
        Ok(())
    }
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
    #[error(
        "agent.tool_budget_exhausted: invocation tool call budget is exhausted (max {max_calls})"
    )]
    InvocationBudgetExhausted { max_calls: usize },
    #[error(
        "agent.tool_budget_exhausted: tool `{tool_id}` call budget is exhausted (max {max_calls})"
    )]
    ToolBudgetExhausted { tool_id: ToolId, max_calls: usize },
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tt_domain::models::tool::{
        InvocationToolSnapshot, ToolBinding, ToolChoice, ToolDescriptor, ToolId, ToolInvocation,
        ToolProviderId, ToolSnapshotId, ToolTurnContract,
    };

    use super::{ToolRequestGate, ToolRequestGateError};

    fn binding(tool_id: ToolId, max_calls: Option<usize>) -> ToolBinding {
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
            max_calls,
        )
        .unwrap()
    }

    fn invocation(call_id: &str, tool_id: ToolId) -> ToolInvocation {
        ToolInvocation {
            call_id: call_id.to_string(),
            tool_id,
            arguments: json!({}),
            provider_metadata: Value::Null,
        }
    }

    #[test]
    fn gate_enforces_turn_choice_before_reserving_budget() {
        let first = ToolId::builtin("first").unwrap();
        let second = ToolId::builtin("second").unwrap();
        let snapshot = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("invocation").unwrap(),
            vec![binding(first.clone(), None), binding(second.clone(), None)],
            1,
        )
        .unwrap();
        let forbidden_turn = ToolTurnContract::all(&snapshot, ToolChoice::None).unwrap();
        let allowed_turn = ToolTurnContract::all(&snapshot, ToolChoice::Auto).unwrap();
        let specific_turn =
            ToolTurnContract::all(&snapshot, ToolChoice::Specific(first.clone())).unwrap();
        let other_snapshot = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("other-invocation").unwrap(),
            vec![binding(first.clone(), None)],
            1,
        )
        .unwrap();
        let other_turn = ToolTurnContract::all(&other_snapshot, ToolChoice::Auto).unwrap();
        let mut gate = ToolRequestGate::default();

        assert!(matches!(
            gate.authorize_and_reserve(
                &snapshot,
                &other_turn,
                &invocation("snapshot", first.clone())
            ),
            Err(ToolRequestGateError::TurnSnapshotMismatch { .. })
        ));
        assert!(matches!(
            gate.authorize_and_reserve(
                &snapshot,
                &forbidden_turn,
                &invocation("none", first.clone())
            ),
            Err(ToolRequestGateError::ToolChoiceNone { .. })
        ));
        assert!(matches!(
            gate.authorize_and_reserve(&snapshot, &specific_turn, &invocation("specific", second)),
            Err(ToolRequestGateError::ToolChoiceSpecific { .. })
        ));
        assert!(matches!(
            gate.authorize_and_reserve(
                &snapshot,
                &allowed_turn,
                &invocation("unknown", ToolId::builtin("unknown").unwrap())
            ),
            Err(ToolRequestGateError::ToolNotInSnapshot { .. })
        ));
        gate.authorize_and_reserve(
            &snapshot,
            &allowed_turn,
            &invocation("allowed", first.clone()),
        )
        .unwrap();
        assert!(matches!(
            gate.authorize_and_reserve(&snapshot, &allowed_turn, &invocation("over", first)),
            Err(ToolRequestGateError::InvocationBudgetExhausted { max_calls: 1 })
        ));
    }

    #[test]
    fn gate_counts_canonical_tool_ids_at_the_exact_per_tool_boundary() {
        let builtin = ToolId::builtin("search").unwrap();
        let external =
            ToolId::new(&ToolProviderId::parse("mcp/server").unwrap(), "search").unwrap();
        let snapshot = InvocationToolSnapshot::try_new(
            ToolSnapshotId::parse("invocation").unwrap(),
            vec![
                binding(builtin.clone(), Some(1)),
                binding(external.clone(), Some(1)),
            ],
            3,
        )
        .unwrap();
        let turn = ToolTurnContract::all(&snapshot, ToolChoice::Auto).unwrap();
        let mut gate = ToolRequestGate::default();

        gate.authorize_and_reserve(&snapshot, &turn, &invocation("builtin", builtin.clone()))
            .unwrap();
        assert!(matches!(
            gate.authorize_and_reserve(&snapshot, &turn, &invocation("builtin-2", builtin)),
            Err(ToolRequestGateError::ToolBudgetExhausted { max_calls: 1, .. })
        ));
        gate.authorize_and_reserve(&snapshot, &turn, &invocation("external", external))
            .unwrap();
    }
}
