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

    let mut base_dir = resolve_base_data_root(
        dirs::data_local_dir(),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        std::env::current_dir().ok(),
        std::env::temp_dir(),
    );
    base_dir.push("soziopolis_lingq_tool");
    ensure_dir(&base_dir)
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}

pub fn database_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("soziopolis_lingq_tool.db"))
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

pub fn browse_cache_dir() -> Result<PathBuf> {
    ensure_dir(&data_dir()?.join("browse_cache"))
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

fn resolve_base_data_root(
    platform_dir: Option<PathBuf>,
    env_dir: Option<PathBuf>,
    cwd: Option<PathBuf>,
    temp_dir: PathBuf,
) -> PathBuf {
    platform_dir.or(env_dir).or(cwd).unwrap_or(temp_dir)
}

fn ensure_dir(path: &PathBuf) -> Result<PathBuf> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    Ok(path.clone())
}

#[cfg(test)]
mod tests {
    use super::resolve_base_data_root;
    use std::path::PathBuf;

    #[test]
    fn resolve_base_data_root_prefers_platform_dir() {
        let platform = PathBuf::from(r"C:\Users\Alice\AppData\Local");
        let env_dir = PathBuf::from(r"D:\Fallback");
        let cwd = PathBuf::from(r"E:\Workspace");

        let resolved = resolve_base_data_root(
            Some(platform.clone()),
            Some(env_dir),
            Some(cwd),
            PathBuf::from(r"F:\Temp"),
        );

        assert_eq!(resolved, platform);
    }

    #[test]
    fn resolve_base_data_root_falls_back_without_hardcoded_user_path() {
        let env_dir = PathBuf::from(r"D:\LocalAppData");
        let cwd = PathBuf::from(r"E:\Workspace");

        let resolved = resolve_base_data_root(
            None,
            Some(env_dir.clone()),
            Some(cwd),
            PathBuf::from(r"F:\Temp"),
        );

        assert_eq!(resolved, env_dir);
        assert!(!resolved.to_string_lossy().contains(r"\Users\Admin\"));
    }
}
