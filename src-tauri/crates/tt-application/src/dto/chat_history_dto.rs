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
