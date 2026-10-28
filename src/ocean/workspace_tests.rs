use super::*;
use egui_dock::{SurfaceIndex, TabInsert};

#[test]
fn dock_edit_hide_restore_preserves_position_and_user_closures() {
    let mut workspaces = Workspaces {
        active: WorkspaceId::Shallow,
        ..Default::default()
    };
    let other = workspaces.items[WorkspaceId::Offshore.index()]
        .layout
        .clone();
    let mut optics = OpticsConfig::beach_scene();
    let ws = workspaces.current();
    ws.prepare(&optics);
    // User docks nearshore foam below the solver rather than sharing its tabs.
    let source = ws.dock.find_tab(&Panel::ShoreFoam).unwrap();
    let target = ws.dock.find_tab(&Panel::Shore).unwrap();
    ws.dock.move_tab(
        source,
        (
            SurfaceIndex::main(),
            target.1,
            TabInsert::Split(Split::Below),
        ),
    );
    ws.capture();
    let custom = ws.layout.clone();
    optics.shore.enabled = false;
    ws.prepare(&optics);
    assert!(ws.dock.find_tab(&Panel::ShoreFoam).is_none());
    assert!(ws.dock.find_tab(&Panel::Shore).is_some()); // Enable entry survives.
    ws.capture();
    optics.shore.enabled = true;
    ws.prepare(&optics);
    assert_eq!(
        Layout::from_dock(ws.dock.main_surface(), NodeIndex::root()).unwrap(),
        custom.filtered(&|p| p.available(&optics)).unwrap()
    );
    // Explicit close is not resurrected on a later enable transition.
    let position = ws.dock.find_tab(&Panel::ShoreFoam).unwrap();
    ws.dock.remove_tab(position);
    ws.capture();
    optics.shore.enabled = false;
    ws.prepare(&optics);
    optics.shore.enabled = true;
    ws.prepare(&optics);
    assert!(ws.dock.find_tab(&Panel::ShoreFoam).is_none());
    workspaces.reset_current();
    assert_eq!(
        workspaces.items[WorkspaceId::Offshore.index()].layout,
        other
    );
}

#[test]
fn edits_while_panel_is_hidden_do_not_lose_it_or_undo_new_splits() {
    let mut ws = Workspace::new(default_layout(WorkspaceId::Shallow));
    let mut optics = OpticsConfig::beach_scene();
    ws.prepare(&optics);
    let source = ws.dock.find_tab(&Panel::ShoreFoam).unwrap();
    let target = ws.dock.find_tab(&Panel::Shore).unwrap();
    ws.dock.move_tab(
        source,
        (
            SurfaceIndex::main(),
            target.1,
            TabInsert::Split(Split::Below),
        ),
    );
    ws.capture();
    optics.shore.enabled = false;
    ws.prepare(&optics);
    ws.add(Panel::Performance);
    let source = ws.dock.find_tab(&Panel::Performance).unwrap();
    let target = ws.dock.find_tab(&Panel::View).unwrap();
    ws.dock.move_tab(
        source,
        (
            SurfaceIndex::main(),
            target.1,
            TabInsert::Split(Split::Below),
        ),
    );
    ws.capture();
    optics.shore.enabled = true;
    ws.prepare(&optics);
    assert!(ws.dock.find_tab(&Panel::ShoreFoam).is_some());
    let performance = ws.dock.find_tab(&Panel::Performance).unwrap();
    let view = ws.dock.find_tab(&Panel::View).unwrap();
    assert_eq!(performance.1.parent(), view.1.parent());
    let before = ws.layout.panels().len();
    ws.add(Panel::Performance);
    assert_eq!(ws.layout.panels().len(), before);
}

#[test]
fn layout_input_repairs_unknown_duplicates_and_missing_view_but_rejects_invalid_splits() {
    let layout: Layout = serde_json::from_str(
        r#"{"kind":"tabs","tabs":["obsolete","shore","shore"],"active":"obsolete"}"#,
    )
    .unwrap();
    let valid = layout.validate().unwrap();
    assert_eq!(
        valid.panels().iter().filter(|p| **p == Panel::View).count(),
        1
    );
    assert_eq!(
        valid
            .panels()
            .iter()
            .filter(|p| **p == Panel::Shore)
            .count(),
        1
    );
    assert!(!valid.contains(Panel::Unknown));
    let invalid = Layout::split(
        Axis::Horizontal,
        1.1,
        Layout::tabs(vec![Panel::View]),
        Layout::tabs(vec![Panel::Shore]),
    );
    assert!(invalid.validate().is_err());
}
