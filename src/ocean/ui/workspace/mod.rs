//! UI-only workspaces. The canonical tree keeps unavailable tabs; Dock edits are
//! merged back by stable panel anchors, never by transient heap node indices.
use crate::ocean::parameters::optics::OpticsConfig;
use egui_dock::{DockState, Node, NodeIndex, Split, TabIndex, Tree};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceId {
    #[default]
    Scene,
    Shallow,
    Offshore,
    Shading,
    Configurations,
}
impl WorkspaceId {
    pub const ALL: [Self; 5] = [
        Self::Scene,
        Self::Shallow,
        Self::Offshore,
        Self::Shading,
        Self::Configurations,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|x| *x == self).unwrap()
    }
    pub fn name(self) -> &'static str {
        ["场景", "浅水", "远海", "着色", "配置"][self.index()]
    }
    pub fn key(self) -> &'static str {
        ["scene", "shallow", "offshore", "shading", "configurations"][self.index()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    View,
    Scene,
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
    FoamMaterial,
    Surface,
    SprayAppearance,
    Configurations,
    Performance,
    #[serde(other)]
    Unknown,
}
impl Panel {
    pub const ALL: [Self; 17] = [
        Self::View,
        Self::Scene,
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
        Self::FoamMaterial,
        Self::Surface,
        Self::SprayAppearance,
        Self::Configurations,
        Self::Performance,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::View => "渲染视图",
            Self::Scene => "场景与地形",
            Self::Model => "模型与区域",
            Self::Colliders => "碰撞体",
            Self::Shore => "SWE 与输入",
            Self::ShoreFoam => "近岸泡沫模拟",
            Self::Spray => "浪花模拟",
            Self::Ocean => "波谱与波形",
            Self::OffshoreFoam => "远海泡沫",
            Self::Optics => "水体与天空",
            Self::Caustics => "焦散",
            Self::Refraction => "折射",
            Self::FoamMaterial => "泡沫外观",
            Self::Surface => "表面与沙地",
            Self::SprayAppearance => "浪花外观",
            Self::Configurations => "配置管理",
            Self::Performance => "性能",
            Self::Unknown => "未知面板",
        }
    }
    pub fn available(self, optics: &OpticsConfig) -> bool {
        match self {
            Self::Colliders | Self::ShoreFoam => optics.swe_enabled(),
            Self::Spray => optics.swe_enabled(),
            Self::SprayAppearance => optics.swe_enabled() && optics.spray.enabled,
            Self::Unknown => false,
            // Enable controls and shared foam appearance remain reachable.
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Layout {
    Tabs {
        tabs: Vec<Panel>,
        active: Panel,
    },
    Split {
        axis: Axis,
        fraction: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}
impl Layout {
    fn tabs(tabs: Vec<Panel>) -> Self {
        Self::Tabs {
            active: tabs[0],
            tabs,
        }
    }
    fn split(axis: Axis, fraction: f32, first: Self, second: Self) -> Self {
        Self::Split {
            axis,
            fraction,
            first: Box::new(first),
            second: Box::new(second),
        }
    }
    fn panels(&self) -> Vec<Panel> {
        match self {
            Self::Tabs { tabs, .. } => tabs.clone(),
            Self::Split { first, second, .. } => [first.panels(), second.panels()].concat(),
        }
    }
    /// One-time migration keeps existing splits, active tabs and closed panels.
    fn add_water_effect_tabs(&mut self) {
        fn insert(layout: &mut Layout, panel: Panel) -> bool {
            match layout {
                Layout::Tabs { tabs, .. } if tabs.contains(&Panel::Optics) => {
                    tabs.push(panel);
                    true
                }
                Layout::Split { first, second, .. } => {
                    insert(first, panel) || insert(second, panel)
                }
                _ => false,
            }
        }
        for panel in [Panel::Caustics, Panel::Refraction] {
            if !self.contains(panel) && !insert(self, panel) {
                self.add(panel);
            }
        }
    }
    fn contains(&self, panel: Panel) -> bool {
        match self {
            Self::Tabs { tabs, .. } => tabs.contains(&panel),
            Self::Split { first, second, .. } => first.contains(panel) || second.contains(panel),
        }
    }
    fn filtered(&self, keep: &impl Fn(Panel) -> bool) -> Option<Self> {
        match self {
            Self::Tabs { tabs, active } => {
                let tabs: Vec<_> = tabs.iter().copied().filter(|p| keep(*p)).collect();
                let first = *tabs.first()?;
                Some(Self::Tabs {
                    active: if tabs.contains(active) {
                        *active
                    } else {
                        first
                    },
                    tabs,
                })
            }
            Self::Split {
                axis,
                fraction,
                first,
                second,
            } => match (first.filtered(keep), second.filtered(keep)) {
                (Some(a), Some(b)) => Some(Self::split(*axis, *fraction, a, b)),
                (a, b) => a.or(b),
            },
        }
    }
    /// Reject unbounded/invalid trees before constructing egui_dock's heap tree.
    pub fn validate(self) -> Result<Self, String> {
        fn check(node: &Layout, depth: usize, leaves: &mut usize) -> Result<(), String> {
            if depth > 16 {
                return Err("布局层级超过 16".into());
            }
            match node {
                Layout::Tabs { tabs, .. } => {
                    *leaves += 1;
                    if *leaves > 64 || tabs.len() > 64 {
                        return Err("布局面板数量过多".into());
                    }
                }
                Layout::Split {
                    fraction,
                    first,
                    second,
                    ..
                } => {
                    if !fraction.is_finite() || !(0.0..1.0).contains(fraction) || *fraction == 0.0 {
                        return Err("布局分割比例无效".into());
                    }
                    check(first, depth + 1, leaves)?;
                    check(second, depth + 1, leaves)?;
                }
            }
            Ok(())
        }
        fn clean(node: Layout, seen: &mut HashSet<Panel>) -> Option<Layout> {
            match node {
                Layout::Tabs { tabs, active } => {
                    let tabs: Vec<_> = tabs
                        .into_iter()
                        .filter(|p| *p != Panel::Unknown && seen.insert(*p))
                        .collect();
                    let first = *tabs.first()?;
                    Some(Layout::Tabs {
                        active: if tabs.contains(&active) {
                            active
                        } else {
                            first
                        },
                        tabs,
                    })
                }
                Layout::Split {
                    axis,
                    fraction,
                    first,
                    second,
                } => match (clean(*first, seen), clean(*second, seen)) {
                    (Some(a), Some(b)) => Some(Layout::split(axis, fraction, a, b)),
                    (a, b) => a.or(b),
                },
            }
        }
        check(&self, 0, &mut 0)?;
        let mut result =
            clean(self, &mut HashSet::new()).unwrap_or_else(|| Self::tabs(vec![Panel::View]));
        if !result.contains(Panel::View) {
            result = Self::split(Axis::Horizontal, 0.3, result, Self::tabs(vec![Panel::View]));
        }
        Ok(result)
    }
    fn dock(&self) -> DockState<Panel> {
        fn fill(layout: &Layout, tree: &mut Tree<Panel>, index: NodeIndex) {
            match layout {
                Layout::Tabs { tabs, active } => {
                    tree[index] = Node::leaf_with(tabs.clone());
                    tree[index].get_leaf_mut().unwrap().active =
                        TabIndex(tabs.iter().position(|p| p == active).unwrap_or(0));
                }
                Layout::Split {
                    axis,
                    fraction,
                    first,
                    second,
                } => {
                    let direction = if *axis == Axis::Horizontal {
                        Split::Right
                    } else {
                        Split::Below
                    };
                    let [a, b] = tree.split_tabs(index, direction, *fraction, vec![Panel::View]);
                    fill(first, tree, a);
                    fill(second, tree, b);
                }
            }
        }
        let mut dock = DockState::new(vec![Panel::View]);
        fill(self, dock.main_surface_mut(), NodeIndex::root());
        dock
    }
    fn from_dock(tree: &Tree<Panel>, index: NodeIndex) -> Option<Self> {
        match &tree[index] {
            Node::Leaf(leaf) => Some(Self::Tabs {
                tabs: leaf.tabs.clone(),
                active: *leaf.tabs.get(leaf.active.0).or(leaf.tabs.first())?,
            }),
            Node::Horizontal(split) | Node::Vertical(split) => {
                let first = Self::from_dock(tree, index.left())?;
                let second = Self::from_dock(tree, index.right())?;
                Some(Self::split(
                    if tree[index].is_horizontal() {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    },
                    split.fraction.clamp(0.01, 0.99),
                    first,
                    second,
                ))
            }
            Node::Empty => None,
        }
    }
    fn leaf_with(&mut self, panel: Panel) -> Option<&mut Self> {
        match self {
            Self::Tabs { tabs, .. } if tabs.contains(&panel) => Some(self),
            Self::Split { first, second, .. } => {
                first.leaf_with(panel).or_else(|| second.leaf_with(panel))
            }
            _ => None,
        }
    }
    fn activate(&mut self, panel: Panel) {
        if let Some(Self::Tabs { active, .. }) = self.leaf_with(panel) {
            *active = panel;
        }
    }
    fn add(&mut self, panel: Panel) {
        if self.contains(panel) {
            self.activate(panel);
            return;
        }
        let neighbor = self.panels().into_iter().find(|p| *p != Panel::View);
        if let Some(Self::Tabs { tabs, active }) = neighbor.and_then(|p| self.leaf_with(p)) {
            tabs.push(panel);
            *active = panel;
        } else {
            *self = Self::split(Axis::Horizontal, 0.3, Self::tabs(vec![panel]), self.clone());
        }
    }
    /// Smallest surviving subtree containing the old sibling's stable anchors.
    fn anchored_subtree(&mut self, anchors: &[Panel]) -> &mut Self {
        let direction = match &*self {
            Self::Split { first, second, .. } => {
                let a = anchors.iter().any(|p| first.contains(*p));
                let b = anchors.iter().any(|p| second.contains(*p));
                if a != b { Some(a) } else { None }
            }
            _ => None,
        };
        match (self, direction) {
            (Self::Split { first, .. }, Some(true)) => first.anchored_subtree(anchors),
            (Self::Split { second, .. }, Some(false)) => second.anchored_subtree(anchors),
            (node, _) => node,
        }
    }
}

#[derive(Clone)]
struct Anchor {
    axis: Axis,
    fraction: f32,
    was_first: bool,
    siblings: Vec<Panel>,
}
/// Reinsert only unavailable panels, not panels the user explicitly closed.
fn preserve_hidden(
    old: &Layout,
    edited: &mut Layout,
    keep: &impl Fn(Panel) -> bool,
    ancestors: &mut Vec<Anchor>,
) {
    match old {
        Layout::Tabs { tabs, active } => {
            let hidden: Vec<_> = tabs.iter().copied().filter(|p| !keep(*p)).collect();
            if hidden.is_empty() {
                return;
            }
            if let Some(neighbor) = tabs.iter().find(|p| keep(**p) && edited.contains(**p)) {
                if let Some(Layout::Tabs { tabs: target, .. }) = edited.leaf_with(*neighbor) {
                    for p in hidden {
                        let old_index = tabs.iter().position(|x| *x == p).unwrap();
                        let next = tabs[old_index + 1..]
                            .iter()
                            .find_map(|x| target.iter().position(|t| t == x));
                        target.insert(next.unwrap_or(target.len()), p);
                    }
                }
                return;
            }
            let hidden_active = if hidden.contains(active) {
                *active
            } else {
                hidden[0]
            };
            let leaf = Layout::Tabs {
                tabs: hidden,
                active: hidden_active,
            };
            for anchor in ancestors.iter().rev() {
                let survivors: Vec<_> = anchor
                    .siblings
                    .iter()
                    .copied()
                    .filter(|p| edited.contains(*p))
                    .collect();
                if !survivors.is_empty() {
                    let target = edited.anchored_subtree(&survivors);
                    let sibling = target.clone();
                    *target = if anchor.was_first {
                        Layout::split(anchor.axis, anchor.fraction, leaf, sibling)
                    } else {
                        Layout::split(anchor.axis, anchor.fraction, sibling, leaf)
                    };
                    return;
                }
            }
            for panel in leaf.panels() {
                edited.add(panel);
            }
        }
        Layout::Split {
            axis,
            fraction,
            first,
            second,
        } => {
            ancestors.push(Anchor {
                axis: *axis,
                fraction: *fraction,
                was_first: true,
                siblings: second.panels(),
            });
            preserve_hidden(first, edited, keep, ancestors);
            ancestors.pop();
            ancestors.push(Anchor {
                axis: *axis,
                fraction: *fraction,
                was_first: false,
                siblings: first.panels(),
            });
            preserve_hidden(second, edited, keep, ancestors);
            ancestors.pop();
        }
    }
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
        WorkspaceId::Scene => stacked(Panel::Scene, vec![Panel::Model, Panel::Colliders], 0.32),
        WorkspaceId::Shallow => stacked(Panel::Shore, vec![Panel::ShoreFoam, Panel::Spray], 0.48),
        WorkspaceId::Offshore => stacked(Panel::Ocean, vec![Panel::OffshoreFoam], 0.68),
        WorkspaceId::Shading => Layout::split(
            Axis::Vertical,
            0.58,
            Layout::tabs(vec![Panel::Optics, Panel::Caustics, Panel::Refraction]),
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

pub struct Workspace {
    pub layout: Layout,
    pub dock: DockState<Panel>,
    projection: Layout,
    available: HashSet<Panel>,
}
impl Workspace {
    pub fn new(layout: Layout) -> Self {
        Self {
            dock: layout.dock(),
            projection: layout.clone(),
            layout,
            available: Panel::ALL.into_iter().collect(),
        }
    }
    pub fn prepare(&mut self, optics: &OpticsConfig) {
        let available: HashSet<_> = Panel::ALL
            .into_iter()
            .filter(|p| p.available(optics))
            .collect();
        if available != self.available {
            self.available = available;
            self.rebuild();
        }
    }
    fn rebuild(&mut self) {
        self.projection = self
            .layout
            .filtered(&|p| self.available.contains(&p))
            .unwrap_or_else(|| Layout::tabs(vec![Panel::View]));
        self.dock = self.projection.dock();
    }
    pub fn capture(&mut self) {
        let Some(edited) = Layout::from_dock(self.dock.main_surface(), NodeIndex::root()) else {
            return;
        };
        if edited != self.projection {
            let mut canonical = edited.clone();
            preserve_hidden(
                &self.layout,
                &mut canonical,
                &|p| self.available.contains(&p),
                &mut Vec::new(),
            );
            self.layout = canonical;
            self.projection = edited;
        }
    }
    pub fn add(&mut self, panel: Panel) {
        self.layout.add(panel);
        self.rebuild();
    }
    pub fn contains(&self, panel: Panel) -> bool {
        self.layout.contains(panel)
    }
}

pub struct Workspaces {
    pub active: WorkspaceId,
    pub items: [Workspace; 5],
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

#[cfg(test)]
mod tests;

pub(in crate::ocean) mod store;
