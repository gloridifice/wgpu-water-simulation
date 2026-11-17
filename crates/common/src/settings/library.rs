use super::Settings;
use serde::{Deserialize, Serialize};
pub const DEFAULT_CONFIGURATION_NAME: &str = "Default";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NamedConfiguration<T> {
    name: String,
    settings: T,
}

impl<T> NamedConfiguration<T> {
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The original configuration is immutable, independent of the working session.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfigurationLibrary<T> {
    default: T,
    profiles: Vec<NamedConfiguration<T>>,
    /// Session selection; independent of the manually saved parameter snapshots.
    #[serde(default)]
    active: Option<usize>,
}

impl<T: Settings> ConfigurationLibrary<T> {
    pub fn new(default: T) -> Self {
        Self {
            default,
            profiles: Vec::new(),
            active: None,
        }
    }

    pub fn profiles(&self) -> &[NamedConfiguration<T>] {
        &self.profiles
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn select(&mut self, index: Option<usize>) -> Result<(), String> {
        self.get(index)?;
        self.active = index;
        Ok(())
    }

    /// `None` identifies the immutable default; indices identify user profiles.
    pub fn get(&self, index: Option<usize>) -> Result<&T, String> {
        match index {
            None => Ok(&self.default),
            Some(index) => self
                .profiles
                .get(index)
                .map(|p| &p.settings)
                .ok_or_else(|| "Configuration not found".into()),
        }
    }

    fn check_name(&self, name: &str, except: Option<usize>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a configuration name".into());
        }
        if name == DEFAULT_CONFIGURATION_NAME
            || self
                .profiles
                .iter()
                .enumerate()
                .any(|(i, p)| Some(i) != except && p.name == name)
        {
            return Err("Configuration name already exists; choose another".into());
        }
        Ok(name.to_owned())
    }

    pub fn save_as(&mut self, name: &str, settings: T) -> Result<usize, String> {
        let name = self.check_name(name, None)?;
        settings.validate()?;
        let index = self.profiles.len();
        self.profiles.push(NamedConfiguration { name, settings });
        self.active = Some(index);
        Ok(index)
    }

    pub fn rename(&mut self, index: Option<usize>, name: &str) -> Result<(), String> {
        let index = index.ok_or("The default configuration cannot be renamed")?;
        let name = self.check_name(name, Some(index))?;
        self.profiles
            .get_mut(index)
            .ok_or("Configuration not found")?
            .name = name;
        Ok(())
    }

    pub fn overwrite(&mut self, index: Option<usize>, settings: T) -> Result<(), String> {
        let index = index.ok_or("The default configuration cannot be overwritten")?;
        settings.validate()?;
        self.profiles
            .get_mut(index)
            .ok_or("Configuration not found")?
            .settings = settings;
        Ok(())
    }

    pub fn delete(&mut self, index: Option<usize>) -> Result<(), String> {
        let index = index.ok_or("The default configuration cannot be deleted")?;
        if index >= self.profiles.len() {
            return Err("Configuration not found".into());
        }
        self.profiles.remove(index);
        self.active = self.active.and_then(|active| match active.cmp(&index) {
            std::cmp::Ordering::Less => Some(active),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(active - 1),
        });
        Ok(())
    }

    pub(super) fn restore_selection(&mut self) {
        // A stale selection must not discard otherwise valid user configurations.
        if self
            .active
            .is_some_and(|index| index >= self.profiles.len())
        {
            self.active = None;
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        self.default.validate()?;
        for (index, profile) in self.profiles.iter().enumerate() {
            // A previously saved user profile may have the newly reserved display name.
            // Preserve it on load without allowing new profiles to take that name.
            if profile.name == DEFAULT_CONFIGURATION_NAME {
                if self
                    .profiles
                    .iter()
                    .enumerate()
                    .any(|(other, p)| other != index && p.name == profile.name)
                {
                    return Err("Configuration name already exists; choose another".into());
                }
            } else if self.check_name(&profile.name, Some(index))? != profile.name {
                return Err("Configuration names cannot have leading or trailing spaces".into());
            }
            profile.settings.validate()?;
        }
        Ok(())
    }
}
