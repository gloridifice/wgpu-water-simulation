//! Ocean workspace registry and defaults. Canonical Dock mechanics live in common.
use crate::ocean::parameters::optics::OpticsConfig;
pub use common::ui::layout::Axis;
#[cfg(test)]
use egui_dock::{NodeIndex, Split};
use serde::{Deserialize, Serialize};
pub type Layout = common::ui::layout::Layout<Panel>;
pub struct Workspace {
    inner: common::ui::layout::Workspace<Panel>,
}
impl std::ops::Deref for Workspace {
    type Target = common::ui::layout::Workspace<Panel>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl std::ops::DerefMut for Workspace {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl Workspace {
    pub fn new(layout: Layout) -> Self {
        Self {
            inner: common::ui::layout::Workspace::new(layout),
        }
    }
    pub fn prepare(&mut self, optics: &OpticsConfig) {
        self.inner.prepare(|p| p.available(optics));
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceId {
    #[default]
    Scene,
    Shallow,
    Offshore,
    AnimatedSdf,
    Shading,
    Configurations,
}
impl WorkspaceId {
    pub const ALL: [Self; 6] = [
        Self::Scene,
        Self::Shallow,
        Self::Offshore,
        Self::AnimatedSdf,
        Self::Shading,
        Self::Configurations,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|x| *x == self).unwrap()
    }
    pub fn name(self) -> &'static str {
        [
            "Scene",
            "Shallow Water",
            "Offshore",
            "Animated SDF Ripples",
            "Shading",
            "Configurations",
        ][self.index()]
    }
    pub fn key(self) -> &'static str {
        [
            "scene",
            "shallow",
            "offshore",
            "animated_sdf",
            "shading",
            "configurations",
        ][self.index()]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    View,
    Scene,
    Lake,
    AnimatedSdf,
    Model,
    Colliders,
    Shore,
    ShoreFoam,
    Spray,
    Ocean,
    OffshoreFoam,
    Optics,
    Caustics,
    Refraction,
    Reflection,
    FoamMaterial,
    Surface,
    SprayAppearance,
    Configurations,
    Performance,
    #[serde(other)]
    Unknown,
}
impl Panel {
    pub const ALL: [Self; 20] = [
        Self::View,
        Self::Scene,
        Self::Lake,
        Self::AnimatedSdf,
        Self::Model,
        Self::Colliders,
        Self::Shore,
        Self::ShoreFoam,
        Self::Spray,
        Self::Ocean,
        Self::OffshoreFoam,
        Self::Optics,
        Self::Caustics,
        Self::Refraction,
        Self::Reflection,
        Self::FoamMaterial,
        Self::Surface,
        Self::SprayAppearance,
        Self::Configurations,
        Self::Performance,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::View => "Render View",
            Self::Scene => "Scene and Terrain",
            Self::Lake => "Lake Terrain",
            Self::AnimatedSdf => "Animated SDF Ripples",
            Self::Model => "Model and Domain",
            Self::Colliders => "Colliders",
            Self::Shore => "SWE and Input",
            Self::ShoreFoam => "Nearshore Foam",
            Self::Spray => "Spray",
            Self::Ocean => "Spectrum and Waves",
            Self::OffshoreFoam => "Offshore Foam",
            Self::Optics => "Water and Sky",
            Self::Caustics => "Caustics",
            Self::Refraction => "Refraction",
            Self::Reflection => "SSR",
            Self::FoamMaterial => "Foam Appearance",
            Self::Surface => "Surface and Sand",
            Self::SprayAppearance => "Spray Appearance",
            Self::Configurations => "Configurations",
            Self::Performance => "Performance",
            Self::Unknown => "Unknown Panel",
        }
    }
    pub fn available(self, optics: &OpticsConfig) -> bool {
        match self {
            Self::Lake => optics.lake.enabled,
            Self::Model | Self::Shore | Self::OffshoreFoam | Self::FoamMaterial | Self::Surface => {
                !optics.lake.enabled
            }
            Self::Colliders | Self::ShoreFoam | Self::Spray => optics.swe_enabled(),
            Self::SprayAppearance => optics.swe_enabled() && optics.spray.enabled,
            Self::Unknown => false,
            _ => true,
        }
    }
}
impl common::ui::layout::PanelId for Panel {
    const VIEW: Self = Self::View;
    const UNKNOWN: Self = Self::Unknown;
    const ALL: &'static [Self] = &Self::ALL;
}
pub fn default_layout(id: WorkspaceId) -> Layout {
    let stacked = |top, bottom, fraction| {
        Layout::split(
            Axis::Vertical,
            fraction,
            Layout::tabs(vec![top]),
            Layout::tabs(bottom),
        )
    };
    let parameters = match id {
        WorkspaceId::Scene => stacked(
            Panel::Scene,
            vec![Panel::Lake, Panel::Model, Panel::Colliders],
            0.32,
        ),
        WorkspaceId::Shallow => stacked(Panel::Shore, vec![Panel::ShoreFoam, Panel::Spray], 0.48),
        WorkspaceId::Offshore => stacked(Panel::Ocean, vec![Panel::OffshoreFoam], 0.68),
        WorkspaceId::AnimatedSdf => Layout::tabs(vec![Panel::AnimatedSdf]),
        WorkspaceId::Shading => Layout::split(
            Axis::Vertical,
            0.58,
            Layout::tabs(vec![
                Panel::Optics,
                Panel::Caustics,
                Panel::Refraction,
                Panel::Reflection,
            ]),
            Layout::tabs(vec![
                Panel::FoamMaterial,
                Panel::Surface,
                Panel::SprayAppearance,
            ]),
        ),
        WorkspaceId::Configurations => Layout::tabs(vec![Panel::Configurations]),
    };
    Layout::split(
        Axis::Horizontal,
        0.31,
        parameters,
        Layout::tabs(vec![Panel::View]),
    )
}
pub struct Workspaces {
    pub active: WorkspaceId,
    pub items: [Workspace; WorkspaceId::ALL.len()],
}
impl Default for Workspaces {
    fn default() -> Self {
        Self {
            active: WorkspaceId::Scene,
            items: WorkspaceId::ALL.map(|id| Workspace::new(default_layout(id))),
        }
    }
}
impl Workspaces {
    pub fn current(&mut self) -> &mut Workspace {
        &mut self.items[self.active.index()]
    }
    pub fn reset_current(&mut self) {
        self.items[self.active.index()] = Workspace::new(default_layout(self.active));
    }
}
pub(in crate::ocean) mod store;
#[cfg(test)]
mod tests;
