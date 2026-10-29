//! Application-owned Chinese font installation, independent of charming_egui themes.

pub(in crate::ocean) fn install(context: &egui::Context) {
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
}
