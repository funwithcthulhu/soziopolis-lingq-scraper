use anyhow::Result;

const LINGQ_SERVICE_NAME: &str = "soziopolis_lingq_tool";
const LINGQ_ACCOUNT_NAME: &str = "lingq_api_key";

#[cfg(target_os = "windows")]
use anyhow::Context;

#[cfg(target_os = "windows")]
fn lingq_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(LINGQ_SERVICE_NAME, LINGQ_ACCOUNT_NAME)
        .context("failed to open Windows Credential Manager entry")
}

#[cfg(target_os = "windows")]
pub fn load_lingq_api_key() -> Result<Option<String>> {
    let entry = lingq_entry()?;
    match entry.get_password() {
        Ok(password) => {
            let trimmed = password.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_owned()))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err).context("failed to read LingQ token from Windows Credential Manager"),
    }
}

#[cfg(target_os = "windows")]
pub fn save_lingq_api_key(api_key: &str) -> Result<()> {
    lingq_entry()?
        .set_password(api_key.trim())
        .context("failed to save LingQ token to Windows Credential Manager")
}

#[cfg(target_os = "windows")]
pub fn clear_lingq_api_key() -> Result<()> {
    let entry = lingq_entry()?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => {
            Err(err).context("failed to remove LingQ token from Windows Credential Manager")
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn load_lingq_api_key() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(target_os = "windows"))]
pub fn save_lingq_api_key(_api_key: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn clear_lingq_api_key() -> Result<()> {
    Ok(())
}
