use crate::database::{Database, SharedDatabase};
use anyhow::Result;

#[derive(Clone)]
pub struct AppContext {
    pub db: SharedDatabase,
}

impl AppContext {
    pub fn shared() -> Result<Self> {
        Ok(Self {
            db: Database::shared_default()?,
        })
    }
}
