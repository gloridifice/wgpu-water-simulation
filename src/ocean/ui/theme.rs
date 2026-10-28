//! 从 wgpu_pbr 移植的 egui / egui_dock 主题，随程序内嵌以支持任意启动目录。

#[derive(serde::Deserialize)]
struct EditorTheme {
    egui_style: egui::Style,
    dock_style: egui_dock::Style,
}

/// 安装控件样式与 MiSans 字体，并返回 DockArea 使用的独立样式。
pub(in crate::ocean) fn install(context: &egui::Context) -> egui_dock::Style {
    let theme: EditorTheme = ron::from_str(include_str!("../../../assets/egui_themes/default.ron"))
        .expect("bundled editor theme must match the egui and egui_dock style schemas");
    context.set_theme(egui::ThemePreference::Dark);
    context.set_style(theme.egui_style);

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "MiSans".into(),
        egui::FontData::from_static(include_bytes!("../../../assets/fonts/MiSans-Normal.ttf"))
            .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "MiSans".into());
    context.set_fonts(fonts);

    theme.dock_style
}
