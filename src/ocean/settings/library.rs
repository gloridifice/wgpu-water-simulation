use super::SavedSettings;
use serde::{Deserialize, Serialize};
pub const DEFAULT_CONFIGURATION_NAME: &str = "默认配置";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NamedConfiguration {
    name: String,
    settings: SavedSettings,
}

impl NamedConfiguration {
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The original configuration is a snapshot, never an autosave target.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfigurationLibrary {
    default: SavedSettings,
    profiles: Vec<NamedConfiguration>,
    /// Session selection; independent of the manually saved parameter snapshots.
    #[serde(default)]
    active: Option<usize>,
}

impl ConfigurationLibrary {
    pub fn new(default: SavedSettings) -> Self {
        Self {
            default,
            profiles: Vec::new(),
            active: None,
        }
    }

    pub fn profiles(&self) -> &[NamedConfiguration] {
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
    pub fn get(&self, index: Option<usize>) -> Result<&SavedSettings, String> {
        match index {
            None => Ok(&self.default),
            Some(index) => self
                .profiles
                .get(index)
                .map(|p| &p.settings)
                .ok_or_else(|| "配置不存在".into()),
        }
    }

    fn check_name(&self, name: &str, except: Option<usize>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("请输入配置名称".into());
        }
        if name == DEFAULT_CONFIGURATION_NAME
            || self
                .profiles
                .iter()
                .enumerate()
                .any(|(i, p)| Some(i) != except && p.name == name)
        {
            return Err("配置名称已存在，请使用其他名称".into());
        }
        Ok(name.to_owned())
    }

    pub fn save_as(&mut self, name: &str, settings: SavedSettings) -> Result<usize, String> {
        let name = self.check_name(name, None)?;
        settings.validate()?;
        let index = self.profiles.len();
        self.profiles.push(NamedConfiguration { name, settings });
        self.active = Some(index);
        Ok(index)
    }

    pub fn rename(&mut self, index: Option<usize>, name: &str) -> Result<(), String> {
        let index = index.ok_or("默认配置不可重命名")?;
        let name = self.check_name(name, Some(index))?;
        self.profiles.get_mut(index).ok_or("配置不存在")?.name = name;
        Ok(())
    }

    pub fn overwrite(
        &mut self,
        index: Option<usize>,
        settings: SavedSettings,
    ) -> Result<(), String> {
        let index = index.ok_or("默认配置不可覆盖")?;
        settings.validate()?;
        self.profiles.get_mut(index).ok_or("配置不存在")?.settings = settings;
        Ok(())
    }

    pub fn delete(&mut self, index: Option<usize>) -> Result<(), String> {
        let index = index.ok_or("默认配置不可删除")?;
        if index >= self.profiles.len() {
            return Err("配置不存在".into());
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
            if self.check_name(&profile.name, Some(index))? != profile.name {
                return Err("配置名称不能包含首尾空格".into());
            }
            profile.settings.validate()?;
        }
        Ok(())
    }
}
