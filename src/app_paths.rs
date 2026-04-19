use anyhow::{Context, Result, anyhow};
use std::{path::PathBuf, sync::OnceLock};

static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

pub fn configure_data_dir(path: PathBuf) -> Result<()> {
    ensure_dir(&path)?;
    DATA_DIR_OVERRIDE
        .set(path)
        .map_err(|_| anyhow!("data directory already configured"))?;
    Ok(())
}

pub fn data_dir() -> Result<PathBuf> {
    if let Some(path) = DATA_DIR_OVERRIDE.get() {
        return ensure_dir(path);
    }

    if let Some(path) = portable_data_dir_from_exe() {
        return ensure_dir(&path);
    }

    let mut base_dir =
        dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Admin\AppData\Local"));
    base_dir.push("soziopolis_lingq_tool");
    ensure_dir(&base_dir)
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}

pub fn database_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("soziopolis_lingq_tool.db"))
}

pub fn queue_state_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("queue_state.json"))
}

pub fn logs_dir() -> Result<PathBuf> {
    ensure_dir(&data_dir()?.join("logs"))
}

pub fn app_log_path() -> Result<PathBuf> {
    Ok(logs_dir()?.join("soziopolis-reader.log"))
}

pub fn support_bundles_dir() -> Result<PathBuf> {
    ensure_dir(&data_dir()?.join("support_bundles"))
}

fn portable_data_dir_from_exe() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for folder_name in ["data", "portable_data"] {
        let candidate = exe_dir.join(folder_name);
        if candidate.is_dir() {
            return Some(candidate.join("soziopolis_lingq_tool"));
        }
    }
    None
}

fn ensure_dir(path: &PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    Ok(path.clone())
}
