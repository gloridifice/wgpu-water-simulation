//! Backend-independent egui widgets and lists with embedded or external TOML themes.
//! Fonts, windows, selection state and persistence remain owned by the application.

pub mod lists;
pub mod theme;
pub mod widgets;

pub use lists::{
    ParameterList, ParameterRows, SelectionList, SelectionRows, group, parameter, parameter_table,
};
pub use theme::{CharmingStyle, Theme, ThemeError, ThemeSources};
pub use widgets::{
    PanelButton, ParameterSlider, SliderAppearance, action_row, choices, h1, h2, h3,
};
