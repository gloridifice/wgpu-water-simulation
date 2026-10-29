//! Shared parameter layout and opt-in, full-width panel action buttons.

/// Theme padding and fixed vertical margins, local to panel action buttons.
struct PanelButtonSpacing {
    padding: egui::Margin,
    margin: egui::Margin,
}

impl PanelButtonSpacing {
    fn from_style(style: &egui::Style) -> Self {
        let padding = style.spacing.button_padding;
        Self {
            padding: egui::Margin::symmetric(padding.x as i8, padding.y as i8),
            margin: egui::Margin::symmetric(0, 4),
        }
    }
}

/// A native egui button frame/interaction with centered, wrapped text and
/// theme padding and 4 pt top/bottom margins. Other buttons are untouched.
#[must_use = "Add the button to a Ui or an action_row"]
pub(crate) struct PanelButton {
    text: egui::WidgetText,
    enabled: bool,
}

impl PanelButton {
    pub(crate) fn new(text: impl Into<egui::WidgetText>) -> Self {
        Self {
            text: text.into(),
            enabled: true,
        }
    }

    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl egui::Widget for PanelButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let spacing = PanelButtonSpacing::from_style(ui.style());
        let width = ui.available_width().max(0.0);
        // Keep insets inside even very narrow parent cells.
        let margin_left = f32::from(spacing.margin.left).min(width);
        let margin_right = f32::from(spacing.margin.right).min(width - margin_left);
        let button_width = width - margin_left - margin_right;
        let padding_left = f32::from(spacing.padding.left).min(button_width);
        let padding_right = f32::from(spacing.padding.right).min(button_width - padding_left);
        let mut job = std::sync::Arc::unwrap_or_clone(self.text.into_layout_job(
            ui.style(),
            egui::TextStyle::Button.into(),
            egui::Align::Center,
        ));
        job.wrap.max_width = button_width - padding_left - padding_right;
        job.halign = egui::Align::Center;
        job.justify = false;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let height = (galley.size().y + spacing.padding.sum().y).max(ui.spacing().interact_size.y);
        let (_, outer) = ui.allocate_space(egui::vec2(width, height + spacing.margin.sum().y));
        let rect = egui::Rect::from_min_size(
            outer.min + egui::vec2(margin_left, f32::from(spacing.margin.top)),
            egui::vec2(button_width, height),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down_justified(egui::Align::Center)),
        );
        child.spacing_mut().button_padding = egui::Vec2::ZERO;
        child.add_enabled(self.enabled, |ui: &mut egui::Ui| {
            // egui owns hover/press/focus rendering and keyboard/pointer input.
            let response = ui.add(egui::Button::new(()).min_size(rect.size()));
            let text_rect = egui::Rect::from_min_max(
                rect.min + egui::vec2(padding_left, f32::from(spacing.padding.top)),
                rect.max - egui::vec2(padding_right, f32::from(spacing.padding.bottom)),
            );
            let pos = text_rect.center() - galley.rect.center().to_vec2();
            let color = ui.style().interact(&response).text_color();
            ui.painter()
                .with_clip_rect(ui.clip_rect().intersect(text_rect))
                .galley(pos, galley.clone(), color);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    &galley.job.text,
                )
            });
            response
        })
    }
}

/// Equal-width actions with theme spacing. Below 96 pt per button (excluding
/// its margins), stack the group. Render once and return native responses.
pub(crate) fn action_row<const N: usize>(
    ui: &mut egui::Ui,
    buttons: [PanelButton; N],
) -> [egui::Response; N] {
    assert!(N > 0, "action rows must contain at least one button");
    let gap = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - gap * (N - 1) as f32) / N as f32;
    if width - PanelButtonSpacing::from_style(ui.style()).margin.sum().x < 96.0 {
        buttons.map(|button| ui.add(button))
    } else {
        let mut buttons = buttons.into_iter();
        ui.columns_const(|columns: &mut [egui::Ui; N]| {
            std::array::from_fn(|i| columns[i].add(buttons.next().unwrap()))
        })
    }
}

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
