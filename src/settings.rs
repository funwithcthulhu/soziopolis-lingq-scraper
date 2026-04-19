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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lingq_api_key: String,
    pub lingq_collection_id: Option<i64>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            last_view: "browse".to_owned(),
            browse_section: "essays".to_owned(),
            browse_only_new: true,
            lingq_api_key: String::new(),
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
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            AppSettings::default()
        };

        Ok(Self { path, data })
    }

    pub fn data(&self) -> &AppSettings {
        &self.data
    }

    pub fn legacy_lingq_api_key(&self) -> Option<String> {
        let api_key = self.data.lingq_api_key.trim();
        if api_key.is_empty() {
            None
        } else {
            Some(api_key.to_owned())
        }
    }

    pub fn clear_legacy_lingq_api_key(&mut self) -> Result<()> {
        if self.data.lingq_api_key.is_empty() {
            return Ok(());
        }

        self.data.lingq_api_key.clear();
        self.save()
    }

    pub fn update<F>(&mut self, updater: F) -> Result<()>
    where
        F: FnOnce(&mut AppSettings),
    {
        updater(&mut self.data);
        self.save()
    }

    pub fn save(&self) -> Result<()> {
        let raw =
            serde_json::to_string_pretty(&self.data).context("failed to serialize settings")?;
        std::fs::write(&self.path, raw)
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        Ok(())
    }
}
