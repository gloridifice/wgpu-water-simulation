//! Canonical Dock trees retain conditionally hidden tabs separately from closures.
use egui_dock::{DockState, Node, NodeIndex, Split, TabIndex, Tree};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, hash::Hash};
pub trait PanelId: Copy + Eq + Hash + 'static {
    const VIEW: Self;
    const UNKNOWN: Self;
    const ALL: &'static [Self];
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Layout<P> {
    Tabs {
        tabs: Vec<P>,
        active: P,
    },
    Split {
        axis: Axis,
        fraction: f32,
        first: Box<Layout<P>>,
        second: Box<Layout<P>>,
    },
}
impl<P: PanelId> Layout<P> {
    pub fn tabs(tabs: Vec<P>) -> Self {
        Self::Tabs {
            active: tabs[0],
            tabs,
        }
    }
    pub fn split(axis: Axis, fraction: f32, first: Self, second: Self) -> Self {
        Self::Split {
            axis,
            fraction,
            first: Box::new(first),
            second: Box::new(second),
        }
    }
    pub fn panels(&self) -> Vec<P> {
        match self {
            Self::Tabs { tabs, .. } => tabs.clone(),
            Self::Split { first, second, .. } => [first.panels(), second.panels()].concat(),
        }
    }
    pub fn contains(&self, p: P) -> bool {
        match self {
            Self::Tabs { tabs, .. } => tabs.contains(&p),
            Self::Split { first, second, .. } => first.contains(p) || second.contains(p),
        }
    }
    /// Insert a new tab by a stable neighbor; callers own version migration policy.
    pub fn insert_near(&mut self, neighbor: P, panel: P) {
        if self.contains(panel) {
            return;
        }
        if let Some(Self::Tabs { tabs, .. }) = self.leaf_with(neighbor) {
            tabs.push(panel);
        } else {
            self.add(panel);
        }
    }
    pub fn filtered(&self, keep: &impl Fn(P) -> bool) -> Option<Self> {
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
    pub fn validate(self) -> Result<Self, String> {
        fn check<P>(n: &Layout<P>, depth: usize, leaves: &mut usize) -> Result<(), String> {
            if depth > 16 {
                return Err("布局层级超过 16".into());
            }
            match n {
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
                    if !fraction.is_finite() || !(*fraction > 0.0 && *fraction < 1.0) {
                        return Err("布局分割比例无效".into());
                    }
                    check(first, depth + 1, leaves)?;
                    check(second, depth + 1, leaves)?;
                }
            }
            Ok(())
        }
        fn clean<P: PanelId>(n: Layout<P>, seen: &mut HashSet<P>) -> Option<Layout<P>> {
            match n {
                Layout::Tabs { tabs, active } => {
                    let tabs: Vec<_> = tabs
                        .into_iter()
                        .filter(|p| *p != P::UNKNOWN && seen.insert(*p))
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
            clean(self, &mut HashSet::new()).unwrap_or_else(|| Self::tabs(vec![P::VIEW]));
        if !result.contains(P::VIEW) {
            result = Self::split(Axis::Horizontal, 0.3, result, Self::tabs(vec![P::VIEW]));
        }
        Ok(result)
    }
    pub fn dock(&self) -> DockState<P> {
        fn fill<P: PanelId>(n: &Layout<P>, tree: &mut Tree<P>, index: NodeIndex) {
            match n {
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
                    let dir = if *axis == Axis::Horizontal {
                        Split::Right
                    } else {
                        Split::Below
                    };
                    let [a, b] = tree.split_tabs(index, dir, *fraction, vec![P::VIEW]);
                    fill(first, tree, a);
                    fill(second, tree, b);
                }
            }
        }
        let mut dock = DockState::new(vec![P::VIEW]);
        fill(self, dock.main_surface_mut(), NodeIndex::root());
        dock
    }
    pub fn from_dock(tree: &Tree<P>, index: NodeIndex) -> Option<Self> {
        match &tree[index] {
            Node::Leaf(leaf) => Some(Self::Tabs {
                tabs: leaf.tabs.clone(),
                active: *leaf.tabs.get(leaf.active.0).or(leaf.tabs.first())?,
            }),
            Node::Horizontal(s) | Node::Vertical(s) => Some(Self::split(
                if tree[index].is_horizontal() {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                },
                s.fraction.clamp(0.01, 0.99),
                Self::from_dock(tree, index.left())?,
                Self::from_dock(tree, index.right())?,
            )),
            Node::Empty => None,
        }
    }
    fn leaf_with(&mut self, p: P) -> Option<&mut Self> {
        match self {
            Self::Tabs { tabs, .. } if tabs.contains(&p) => Some(self),
            Self::Split { first, second, .. } => first.leaf_with(p).or_else(|| second.leaf_with(p)),
            _ => None,
        }
    }
    pub fn add(&mut self, p: P) {
        if let Some(Self::Tabs { active, .. }) = self.leaf_with(p) {
            *active = p;
            return;
        }
        let neighbor = self.panels().into_iter().find(|p| *p != P::VIEW);
        if let Some(Self::Tabs { tabs, active }) = neighbor.and_then(|p| self.leaf_with(p)) {
            tabs.push(p);
            *active = p;
        } else {
            *self = Self::split(Axis::Horizontal, 0.3, Self::tabs(vec![p]), self.clone());
        }
    }
    fn anchored_subtree(&mut self, anchors: &[P]) -> &mut Self {
        let dir = match &*self {
            Self::Split { first, second, .. } => {
                let a = anchors.iter().any(|p| first.contains(*p));
                let b = anchors.iter().any(|p| second.contains(*p));
                if a != b { Some(a) } else { None }
            }
            _ => None,
        };
        match (self, dir) {
            (Self::Split { first, .. }, Some(true)) => first.anchored_subtree(anchors),
            (Self::Split { second, .. }, Some(false)) => second.anchored_subtree(anchors),
            (node, _) => node,
        }
    }
}
#[derive(Clone)]
struct Anchor<P> {
    axis: Axis,
    fraction: f32,
    was_first: bool,
    siblings: Vec<P>,
}
fn preserve_hidden<P: PanelId>(
    old: &Layout<P>,
    edited: &mut Layout<P>,
    keep: &impl Fn(P) -> bool,
    ancestors: &mut Vec<Anchor<P>>,
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
            for a in ancestors.iter().rev() {
                let survivors: Vec<_> = a
                    .siblings
                    .iter()
                    .copied()
                    .filter(|p| edited.contains(*p))
                    .collect();
                if !survivors.is_empty() {
                    let target = edited.anchored_subtree(&survivors);
                    let sibling = target.clone();
                    *target = if a.was_first {
                        Layout::split(a.axis, a.fraction, leaf, sibling)
                    } else {
                        Layout::split(a.axis, a.fraction, sibling, leaf)
                    };
                    return;
                }
            }
            for p in leaf.panels() {
                edited.add(p);
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
pub struct Workspace<P> {
    pub layout: Layout<P>,
    pub dock: DockState<P>,
    projection: Layout<P>,
    available: HashSet<P>,
}
impl<P: PanelId> Workspace<P> {
    pub fn new(layout: Layout<P>) -> Self {
        Self {
            dock: layout.dock(),
            projection: layout.clone(),
            layout,
            available: P::ALL.iter().copied().collect(),
        }
    }
    pub fn prepare(&mut self, keep: impl Fn(P) -> bool) {
        let available: HashSet<_> = P::ALL.iter().copied().filter(|p| keep(*p)).collect();
        if available != self.available {
            self.available = available;
            self.rebuild();
        }
    }
    fn rebuild(&mut self) {
        self.projection = self
            .layout
            .filtered(&|p| self.available.contains(&p))
            .unwrap_or_else(|| Layout::tabs(vec![P::VIEW]));
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
    pub fn add(&mut self, p: P) {
        self.layout.add(p);
        self.rebuild();
    }
    pub fn contains(&self, p: P) -> bool {
        self.layout.contains(p)
    }
}
