//! Bundled scene selection. Playground remains the small deterministic verification fixture.
use crate::{
    Error,
    terrain::{BakeSource, Terrain},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenePreset {
    #[default]
    #[serde(alias = "datun")]
    Grandma,
    Playground,
    /// External --scene/--terrain pair; requires explicit paths on subsequent launches.
    Custom,
}
impl ScenePreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grandma => "Grandma’s House",
            Self::Playground => "网格实验场",
            Self::Custom => "外部场景",
        }
    }
    pub fn parse_name(name: &str) -> Option<Self> {
        match name {
            "grandma" => Some(Self::Grandma),
            "playground" => Some(Self::Playground),
            _ => None,
        }
    }
    pub fn load(self) -> Result<(BakeSource, Terrain), Error> {
        let (source, bytes): (BakeSource, &[u8]) = match self {
            Self::Grandma => (
                crate::model::decode(include_bytes!("../assets/grandma.fogscene"))?,
                include_bytes!("../assets/grandma.fogterrain"),
            ),
            Self::Playground => (
                BakeSource::playground(),
                include_bytes!("../assets/playground.fogterrain"),
            ),
            Self::Custom => {
                return Err("外部场景需要显式 --scene PATH --terrain PATH，或选择内置场景".into());
            }
        };
        let terrain = Terrain::decode(bytes, &source)?;
        Ok((source, terrain))
    }
}
