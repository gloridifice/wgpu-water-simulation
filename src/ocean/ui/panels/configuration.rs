//! Compact single-selection configuration controls.
use crate::ocean::settings::{ConfigurationLibrary, DEFAULT_CONFIGURATION_NAME, SavedSettings};

#[derive(Clone, Copy)]
enum Editor {
    New,
    Rename(usize),
    Delete(usize),
}

pub struct ConfigurationPanel {
    pub library: ConfigurationLibrary,
    pub save_now: bool,
    selected: Option<usize>,
    editor: Option<Editor>,
    name: String,
    focus_name: bool,
    previous_selection: Option<Option<usize>>,
    error: Option<String>,
}

impl ConfigurationPanel {
    pub fn new(library: ConfigurationLibrary) -> Self {
        Self {
            selected: library.active(),
            library,
            save_now: false,
            editor: None,
            name: String::new(),
            focus_name: false,
            previous_selection: None,
            error: None,
        }
    }

    pub fn finish_load(&mut self, result: Result<(), String>) {
        let result = result.and_then(|()| self.library.select(self.selected));
        if result.is_err()
            && let Some(previous) = self.previous_selection.take()
        {
            self.selected = previous;
        }
        self.previous_selection = None;
        self.error = result.err();
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        current: SavedSettings,
        persistence_enabled: bool,
    ) -> Option<SavedSettings> {
        let mut load = None;

        let mut clicked = None;
        let mut rename = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("configuration buttons")
                .max_height(260.0)
                .show(ui, |ui| {
                    let row_size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                    if ui
                        .add_sized(
                            row_size,
                            egui::Button::selectable(
                                self.selected.is_none(),
                                DEFAULT_CONFIGURATION_NAME,
                            )
                            .wrap(),
                        )
                        .on_hover_text("只读默认配置。点击恢复全部参数并重置模拟历史。")
                        .clicked()
                    {
                        clicked = Some(None);
                    }
                    for (index, profile) in self.library.profiles().iter().enumerate() {
                        let response = ui
                            .push_id(index, |ui| {
                                ui.add_sized(
                                    row_size,
                                    egui::Button::selectable(
                                        self.selected == Some(index),
                                        profile.name(),
                                    )
                                    .wrap(),
                                )
                            })
                            .inner
                            .on_hover_text("点击载入全部参数并重置模拟历史；右键可重命名。");
                        if response.clicked() {
                            clicked = Some(Some(index));
                        }
                        if persistence_enabled {
                            response.context_menu(|ui| {
                                if ui.button("重命名").clicked() {
                                    rename = Some(index);
                                    ui.close();
                                }
                            });
                        }
                    }
                });
        });
        if let Some(index) = clicked {
            match self.library.get(index) {
                Ok(settings) => {
                    self.previous_selection = Some(self.selected);
                    self.selected = index;
                    self.editor = None;
                    self.error = None;
                    load = Some(settings.clone());
                }
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(index) = rename {
            self.editor = Some(Editor::Rename(index));
            self.name = self.library.profiles()[index].name().to_owned();
            self.focus_name = true;
            self.error = None;
        }

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(persistence_enabled, egui::Button::new("新配置"))
                .on_hover_text("将当前全部参数另存为新配置。")
                .on_disabled_hover_text("本地保存已禁用")
                .clicked()
            {
                self.editor = Some(Editor::New);
                self.name.clear();
                self.focus_name = true;
                self.error = None;
            }
            if ui
                .add_enabled(
                    persistence_enabled && self.selected.is_some(),
                    egui::Button::new("删除配置"),
                )
                .on_disabled_hover_text(if persistence_enabled {
                    "默认配置不可删除"
                } else {
                    "本地保存已禁用"
                })
                .clicked()
                && let Some(index) = self.selected
            {
                self.editor = Some(Editor::Delete(index));
                self.error = None;
            }
        });

        if let Some(editor) = self.editor {
            ui.push_id("configuration editor", |ui| {
                match editor {
                    Editor::New | Editor::Rename(_) => {
                        let input = ui.add(
                            egui::TextEdit::singleline(&mut self.name)
                                .hint_text("配置名称")
                                .desired_width(f32::INFINITY),
                        );
                        if std::mem::take(&mut self.focus_name) {
                            input.request_focus();
                        }
                    }
                    Editor::Delete(index) => {
                        ui.label(format!(
                            "是否删除当前配置“{}”？",
                            self.library.profiles()[index].name()
                        ));
                    }
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(persistence_enabled, egui::Button::new("确定"))
                        .clicked()
                    {
                        let result = match editor {
                            Editor::New => self
                                .library
                                .save_as(&self.name, current.clone())
                                .map(|index| self.selected = Some(index)),
                            Editor::Rename(index) => self.library.rename(Some(index), &self.name),
                            Editor::Delete(index) => self.library.delete(Some(index)).map(|()| {
                                self.selected = None;
                                // Vec::remove shifts later indices; restore the default after deletion.
                                self.previous_selection = Some(None);
                                load = self.library.get(None).ok().cloned();
                            }),
                        };
                        if result.is_ok() {
                            self.editor = None;
                            self.name.clear();
                            self.save_now = true;
                        }
                        self.error = result.err();
                    }
                    if ui.button("取消").clicked() {
                        self.editor = None;
                        self.name.clear();
                        self.error = None;
                    }
                });
            });
        }

        if ui
            .add_enabled(
                persistence_enabled
                    && self.selected.is_some()
                    && self.editor.is_none()
                    && load.is_none(),
                egui::Button::new("保存配置"),
            )
            .on_hover_text("用当前全部参数覆盖所选命名配置；自动保存仅记录工作状态。")
            .on_disabled_hover_text(if persistence_enabled {
                "默认配置只读；请先新建或选择一个命名配置"
            } else {
                "本地保存已禁用"
            })
            .clicked()
        {
            let result = self.library.overwrite(self.selected, current);
            self.save_now |= result.is_ok();
            self.error = result.err();
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        load
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Harness {
        context: egui::Context,
        panel: ConfigurationPanel,
        current: SavedSettings,
        enabled: bool,
        shapes: Vec<egui::epaint::ClippedShape>,
        loaded: Option<SavedSettings>,
    }

    impl Harness {
        fn new(enabled: bool) -> Self {
            let context = egui::Context::default();
            crate::ocean::ui::theme::install(&context);
            let initial = SavedSettings::default();
            Self {
                context,
                panel: ConfigurationPanel::new(ConfigurationLibrary::new(initial.clone())),
                current: initial,
                enabled,
                shapes: Vec::new(),
                loaded: None,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let output = self.context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let Some(settings) =
                            self.panel.ui(ui, self.current.clone(), self.enabled)
                        {
                            self.current = settings.clone();
                            self.loaded = Some(settings);
                            self.panel.finish_load(Ok(()));
                        }
                    });
                },
            );
            self.shapes = output.shapes;
        }

        fn click(&mut self, label: &str) {
            self.click_with(label, egui::PointerButton::Primary);
        }

        fn click_with(&mut self, label: &str, button: egui::PointerButton) {
            fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
                match shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                        Some(text.visual_bounding_rect().center())
                    }
                    egui::epaint::Shape::Vec(shapes) => {
                        shapes.iter().rev().find_map(|s| find(s, label))
                    }
                    _ => None,
                }
            }
            let pos = self
                .shapes
                .iter()
                .rev()
                .find_map(|s| find(&s.shape, label))
                .unwrap_or_else(|| panic!("control not rendered: {label}"));
            for pressed in [true, false] {
                self.frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
            self.frame(vec![]);
        }

        fn create(&mut self, name: &str) {
            self.click("新配置");
            self.frame(vec![egui::Event::Text(name.into())]);
            self.click("确定");
        }
    }

    #[test]
    fn controls_create_save_select_rename_and_confirm_or_cancel_deletion() {
        let mut h = Harness::new(true);
        let default = h.current.clone();
        h.current.optics.depth = 17.0;
        h.current.draft.seed = 123;
        h.frame(vec![]);
        h.click("新配置");
        h.frame(vec![egui::Event::Text("放弃的配置".into())]);
        h.click("取消");
        assert!(h.panel.library.profiles().is_empty());

        h.create("沙滩方案");
        assert_eq!(h.panel.library.get(Some(0)).unwrap(), &h.current);
        assert!(std::mem::take(&mut h.panel.save_now));
        h.current.speed = 0.5;
        h.frame(vec![]);
        h.click("保存配置");
        let saved = h.current.clone();
        assert_eq!(h.panel.library.get(Some(0)).unwrap(), &saved);

        h.click_with("沙滩方案", egui::PointerButton::Secondary);
        h.click("重命名");
        h.frame(vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: Some(egui::Key::A),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            },
            egui::Event::Text("浅水方案".into()),
        ]);
        h.click("确定");
        assert_eq!(h.panel.library.profiles()[0].name(), "浅水方案");

        h.click(DEFAULT_CONFIGURATION_NAME);
        assert_eq!(h.loaded.take().unwrap(), default);
        assert_eq!(h.panel.library.active(), None);
        h.click("浅水方案");
        assert_eq!(h.loaded.take().unwrap(), saved);
        assert_eq!(h.panel.library.active(), Some(0));
        h.create("保留的配置");
        h.click("浅水方案");
        h.click("删除配置");
        h.click("取消");
        assert_eq!(h.panel.library.profiles().len(), 2);
        assert_eq!(h.current, saved);
        h.click("删除配置");
        h.click("确定");
        assert_eq!(h.panel.library.profiles().len(), 1);
        assert_eq!(h.panel.library.profiles()[0].name(), "保留的配置");
        assert_eq!(h.current, default);
        h.click("保留的配置");
        assert_eq!(h.loaded.take().unwrap(), saved);
        assert_eq!(h.panel.selected, Some(0));
    }

    #[test]
    fn restart_restores_selection_even_when_working_parameters_differ() {
        let mut h = Harness::new(true);
        let initial = h.current.clone();
        let mut library = ConfigurationLibrary::new(initial.clone());
        let index = library.save_as("上次使用", initial).unwrap();
        h.panel = ConfigurationPanel::new(library);
        h.current.optics.depth = 17.0;
        h.frame(vec![]);
        assert_eq!(h.panel.selected, Some(index));
        assert!(h.loaded.is_none());
        assert_eq!(h.current.optics.depth, 17.0);

        // A failed load leaves the persisted selection on the previous configuration.
        h.panel.previous_selection = Some(h.panel.selected);
        h.panel.selected = None;
        h.panel.finish_load(Err("测试载入失败".into()));
        assert_eq!(h.panel.selected, Some(index));
        assert_eq!(h.panel.library.active(), Some(index));
    }

    #[test]
    fn default_is_read_only_and_disabled_persistence_still_allows_loading() {
        let mut h = Harness::new(false);
        h.current.optics.depth = 17.0;
        h.frame(vec![]);
        h.click("新配置");
        assert!(h.panel.editor.is_none());
        h.click("删除配置");
        h.click("保存配置");
        assert!(h.panel.library.profiles().is_empty());
        assert!(!h.panel.save_now);
        h.click(DEFAULT_CONFIGURATION_NAME);
        assert_eq!(h.loaded.take().unwrap(), SavedSettings::default());
        h.enabled = true;
        h.click("删除配置");
        h.click("保存配置");
        assert!(h.panel.editor.is_none());
        assert!(!h.panel.save_now);
    }
}
