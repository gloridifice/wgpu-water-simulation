//! Compatibility names for the application's concrete settings schema.
pub use common::settings::DEFAULT_CONFIGURATION_NAME;
pub type NamedConfiguration = common::settings::NamedConfiguration<super::SavedSettings>;
pub type ConfigurationLibrary = common::settings::ConfigurationLibrary<super::SavedSettings>;
