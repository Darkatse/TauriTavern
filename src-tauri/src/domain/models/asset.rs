use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssetCategory {
    Bgm,
    Ambient,
    Blip,
    Live2d,
    Vrm,
    Character,
    Temp,
}

impl AssetCategory {
    pub const ALL: [Self; 7] = [
        Self::Bgm,
        Self::Ambient,
        Self::Blip,
        Self::Live2d,
        Self::Vrm,
        Self::Character,
        Self::Temp,
    ];

    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "bgm" => Some(Self::Bgm),
            "ambient" => Some(Self::Ambient),
            "blip" => Some(Self::Blip),
            "live2d" => Some(Self::Live2d),
            "vrm" => Some(Self::Vrm),
            "character" => Some(Self::Character),
            "temp" => Some(Self::Temp),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bgm => "bgm",
            Self::Ambient => "ambient",
            Self::Blip => "blip",
            Self::Live2d => "live2d",
            Self::Vrm => "vrm",
            Self::Character => "character",
            Self::Temp => "temp",
        }
    }

    pub fn is_temp(self) -> bool {
        self == Self::Temp
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VrmAssetCatalog {
    pub model: Vec<String>,
    pub animation: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum AssetCatalogEntry {
    Files(Vec<String>),
    Vrm(VrmAssetCatalog),
}

pub type AssetCatalog = BTreeMap<String, AssetCatalogEntry>;
