use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ChatHistoryLocator {
    #[serde(rename = "character")]
    Character {
        #[serde(rename = "characterId")]
        character_id: String,
        #[serde(rename = "fileName")]
        file_name: String,
    },
    #[serde(rename = "group")]
    Group {
        #[serde(rename = "chatId")]
        chat_id: String,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CurrentCommitReason {
    #[default]
    Mutation,
    ProviderBarrier,
    GenerationCheckpoint,
    Maintenance,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn frontend_locator_and_reason_shape_is_camel_case() {
        let locator: ChatHistoryLocator = serde_json::from_value(json!({
            "kind": "character",
            "characterId": "Alice#1",
            "fileName": "Story",
        }))
        .expect("deserialize character locator");
        assert_eq!(
            locator,
            ChatHistoryLocator::Character {
                character_id: "Alice#1".to_string(),
                file_name: "Story".to_string(),
            }
        );

        let reason: CurrentCommitReason = serde_json::from_value(json!("generationCheckpoint"))
            .expect("deserialize commit reason");
        assert_eq!(reason, CurrentCommitReason::GenerationCheckpoint);

        let maintenance: CurrentCommitReason =
            serde_json::from_value(json!("maintenance")).expect("deserialize maintenance reason");
        assert_eq!(maintenance, CurrentCommitReason::Maintenance);
    }
}
