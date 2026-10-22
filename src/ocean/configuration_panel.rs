//! Named configuration controls; the viewer applies loaded GPU settings.
use super::settings::{ConfigurationLibrary, DEFAULT_CONFIGURATION_NAME, SavedSettings};

pub struct ConfigurationPanel {
    pub library: ConfigurationLibrary,
    pub save_now: bool,
    selected: Option<usize>,
    new_name: String,
    rename_name: String,
    pub message: Option<Result<String, String>>,
}

impl ConfigurationPanel {
    pub fn new(library: ConfigurationLibrary) -> Self {
        Self {
            library,
            save_now: false,
            selected: None,
            new_name: String::new(),
            rename_name: String::new(),
            message: None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        current: SavedSettings,
        persistence_enabled: bool,
    ) -> Option<SavedSettings> {
        let mut load = None;
        ui.label("配置包含海洋、泡沫、浅水、光学、相机和播放参数，也保留未应用的波谱草稿。");
        ui.label("自动保存仅记住当前工作状态；命名配置需手动另存或覆盖。");
        ui.separator();

        let previous = self.selected;
        egui::ComboBox::from_id_salt("saved configurations")
            .width(ui.available_width())
            .selected_text(
                self.selected
                    .and_then(|i| self.library.profiles().get(i))
                    .map_or(DEFAULT_CONFIGURATION_NAME, |p| p.name()),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.selected, None, DEFAULT_CONFIGURATION_NAME);
                for (index, profile) in self.library.profiles().iter().enumerate() {
                    ui.selectable_value(&mut self.selected, Some(index), profile.name());
                }
            });
        if self.selected != previous {
            self.rename_name = self
                .selected
                .and_then(|i| self.library.profiles().get(i))
                .map_or_else(String::new, |p| p.name().to_owned());
            self.message = None;
        }

        if self.selected.is_none() {
            ui.label("默认配置为首次载入时的配置，只读，可随时恢复。");
        }
        if let Ok(saved) = self.library.get(self.selected) {
            ui.label(if saved == &current {
                "当前参数与所选配置一致"
            } else {
                "当前参数与所选配置不同"
            });
        }
        ui.horizontal_wrapped(|ui| {
            let label = if self.selected.is_none() {
                "恢复默认配置"
            } else {
                "载入配置"
            };
            if ui
                .button(label)
                .on_hover_text("载入全部参数并重置模拟时间、泡沫和浅水历史。")
                .clicked()
            {
                match self.library.get(self.selected) {
                    Ok(settings) => load = Some(settings.clone()),
                    Err(error) => self.message = Some(Err(error)),
                }
            }
            if ui
                .add_enabled(
                    persistence_enabled && self.selected.is_some(),
                    egui::Button::new("覆盖所选配置"),
                )
                .clicked()
            {
                let result = self.library.overwrite(self.selected, current.clone());
                self.save_now |= result.is_ok();
                self.message = Some(result.map(|()| "已更新所选配置".into()));
            }
        });

        ui.add_enabled_ui(persistence_enabled && self.selected.is_some(), |ui| {
            ui.label("配置名称");
            ui.add(egui::TextEdit::singleline(&mut self.rename_name).desired_width(f32::INFINITY));
            if ui.button("重命名").clicked() {
                let result = self.library.rename(self.selected, &self.rename_name);
                self.save_now |= result.is_ok();
                self.message = Some(result.map(|()| "已重命名".into()));
            }
        });

        ui.separator();
        ui.label(egui::RichText::new("另存为新配置").strong());
        ui.add_enabled_ui(persistence_enabled, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("输入新配置名称")
                    .desired_width(f32::INFINITY),
            );
            if ui.button("保存为新配置").clicked() {
                match self.library.save_as(&self.new_name, current) {
                    Ok(index) => {
                        self.selected = Some(index);
                        self.rename_name = self.library.profiles()[index].name().to_owned();
                        self.new_name.clear();
                        self.save_now = true;
                        self.message = Some(Ok("已新增配置".into()));
                    }
                    Err(error) => self.message = Some(Err(error)),
                }
            }
        });
        if !persistence_enabled {
            ui.label("本地保存已禁用；重新启动时去掉 --no-settings 可启用。");
        }
        if let Some(message) = &self.message {
            match message {
                Ok(message) => {
                    ui.label(message);
                }
                Err(error) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            }
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
            super::super::theme::install(&context);
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
                            self.loaded = Some(settings);
                        }
                    });
                },
            );
            self.shapes = output.shapes;
        }

        fn click(&mut self, label: &str) {
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
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
        }
    }

    #[test]
    fn controls_save_rename_overwrite_load_and_restore_default() {
        let mut h = Harness::new(true);
        let default = h.current.clone();
        h.current.optics.depth = 17.0;
        h.current.draft.seed = 123;
        h.frame(vec![]);
        h.click("输入新配置名称");
        h.frame(vec![egui::Event::Text("沙滩方案".into())]);
        h.click("保存为新配置");
        assert_eq!(h.panel.library.get(Some(0)).unwrap(), &h.current);
        assert!(std::mem::take(&mut h.panel.save_now));

        h.frame(vec![]);
        h.click("沙滩方案");
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
        h.click("重命名");
        assert_eq!(h.panel.library.profiles()[0].name(), "浅水方案");
        assert!(std::mem::take(&mut h.panel.save_now));

        h.current.speed = 0.5;
        h.frame(vec![]);
        h.click("覆盖所选配置");
        let saved = h.current.clone();
        assert_eq!(h.panel.library.get(Some(0)).unwrap(), &saved);
        h.current.optics.depth = 5.0;
        h.frame(vec![]);
        h.click("载入配置");
        assert_eq!(h.loaded.take().unwrap(), saved);

        // Open the selector and use the read-only default entry.
        h.frame(vec![]);
        let combo = h
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "浅水方案" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false] {
            h.frame(vec![
                egui::Event::PointerMoved(combo),
                egui::Event::PointerButton {
                    pos: combo,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        h.frame(vec![]);
        h.click(DEFAULT_CONFIGURATION_NAME);
        h.click("恢复默认配置");
        assert_eq!(h.loaded.take().unwrap(), default);
        assert_eq!(h.panel.library.get(None).unwrap(), &default);
    }

    #[test]
    fn disabled_persistence_prevents_save_but_allows_default_restore() {
        let mut h = Harness::new(false);
        h.current.optics.depth = 17.0;
        h.panel.new_name = "不能保存".into();
        h.frame(vec![]);
        h.click("保存为新配置");
        assert!(h.panel.library.profiles().is_empty());
        assert!(!h.panel.save_now);
        h.click("覆盖所选配置");
        h.click("重命名");
        h.click("恢复默认配置");
        assert_eq!(h.loaded.unwrap(), SavedSettings::default());
    }
}
