//! Shared two-column parameter layout.
pub(super) const PARAMETER_NAME_RATIO: f32 = 0.42;
pub(super) const PARAMETER_COLUMN_GAP: f32 = 8.0;

/// All groups use the same ratio; widget minimum sizes cannot widen a column.
pub(crate) fn parameter_table<R>(
    ui: &mut egui::Ui,
    id: &str,
    rows: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let usable = (ui.available_width() - PARAMETER_COLUMN_GAP).max(0.0);
    let widths = [
        usable * PARAMETER_NAME_RATIO,
        usable * (1.0 - PARAMETER_NAME_RATIO),
    ];
    let key = egui::Id::new("parameter table column widths");
    let previous = ui.data_mut(|data| {
        let old = data.get_temp::<[f32; 2]>(key);
        data.insert_temp(key, widths);
        old
    });
    let result = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = (widths[1] - 55.0).max(0.0);
            egui::Grid::new((id, widths[0].to_bits()))
                .num_columns(2)
                .min_col_width(widths[0])
                .spacing(egui::vec2(PARAMETER_COLUMN_GAP, 6.0))
                .striped(true)
                .show(ui, rows)
                .inner
        })
        .inner;
    ui.data_mut(|data| {
        if let Some(old) = previous {
            data.insert_temp(key, old);
        } else {
            data.remove::<[f32; 2]>(key);
        }
    });
    result
}

/// Reserve exactly the column width even if a control has a larger minimum size.
fn parameter_cell<R>(
    ui: &mut egui::Ui,
    id: (&str, &str),
    width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let origin = ui.available_rect_before_wrap().min;
    let bounds = egui::Rect::from_min_size(origin, egui::vec2(width, 100000.0));
    let mut cell = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(bounds)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    cell.set_clip_rect(ui.clip_rect().intersect(bounds));
    cell.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    let value = contents(&mut cell);
    ui.allocate_space(egui::vec2(width, cell.min_size().y));
    value
}

/// Append a name/control row to the enclosing parameter table.
pub(crate) fn parameter<R>(
    ui: &mut egui::Ui,
    name: &str,
    description: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let widths = ui
        .data(|data| data.get_temp::<[f32; 2]>(egui::Id::new("parameter table column widths")))
        .expect("parameter rows must be inside a shared table");
    parameter_cell(ui, (name, "name"), widths[0], |ui| {
        ui.add(egui::Label::new(name).wrap())
            .on_hover_text(description);
    });
    let value = parameter_cell(ui, (name, "control"), widths[1], control);
    ui.end_row();
    value
}

pub(crate) fn group(ui: &mut egui::Ui, title: &str, rows: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).strong());
    parameter_table(ui, title, rows);
}

/// A bounded horizontal exclusive button row, not a row of independent toggles.
pub(crate) fn choices<T: Copy + PartialEq>(ui: &mut egui::Ui, value: &mut T, items: &[(T, &str)]) {
    let gap = 3.0;
    let width = ((ui.available_width() - gap * items.len().saturating_sub(1) as f32)
        / items.len().max(1) as f32)
        .max(1.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (choice, label) in items {
            if ui
                .add_sized(
                    [width, ui.spacing().interact_size.y],
                    egui::Button::selectable(*value == *choice, *label).wrap(),
                )
                .clicked()
            {
                *value = *choice;
            }
        }
    });
}
