use crate::app_paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub last_view: String,
    pub browse_section: String,
    #[serde(default)]
    pub browse_only_new: bool,
    pub lingq_collection_id: Option<i64>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            last_view: "browse".to_owned(),
            browse_section: "essays".to_owned(),
            browse_only_new: true,
            lingq_collection_id: None,
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    data: AppSettings,
}

impl SettingsStore {
    pub fn from_parts(path: PathBuf, data: AppSettings) -> Self {
        Self { path, data }
    }

    pub fn load_default() -> Result<Self> {
        let path = app_paths::settings_path()?;
        Self::load(path)
    }

    pub fn create_default() -> Result<Self> {
        let path = app_paths::settings_path()?;
        Ok(Self {
            path,
            data: AppSettings::default(),
        })
    }

    pub fn load(path: PathBuf) -> Result<Self> {
        let data = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?
        } else {
            AppSettings::default()
        };

        Ok(Self { path, data })
    }

    pub fn data(&self) -> &AppSettings {
        &self.data
    }

    pub fn update<F>(&mut self, updater: F) -> Result<()>
    where
        F: FnOnce(&mut AppSettings),
    {
        updater(&mut self.data);
        self.save()
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let raw =
            serde_json::to_string_pretty(&self.data).context("failed to serialize settings")?;
        std::fs::write(&self.path, raw)
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, SettingsStore};
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{label}_{unique}"))
    }

    #[test]
    fn load_reports_invalid_json_instead_of_silently_resetting() {
        let dir = unique_temp_path("soziopolis_settings_invalid");
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ invalid json").expect("invalid json should be written");

        let error = match SettingsStore::load(path.clone()) {
            Ok(_) => panic!("invalid json should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("failed to parse"));

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = unique_temp_path("soziopolis_settings_save");
        let path = dir.join("nested").join("settings.json");
        let store = SettingsStore::from_parts(path.clone(), AppSettings::default());

        store.save().expect("save should create parent directories");

        assert!(path.exists());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(path.parent().expect("nested parent"));
        let _ = std::fs::remove_dir(dir);
    }
}
