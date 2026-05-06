use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusty_data::connection::{self, ConnectionConfig, ConnectionMode};
use rusty_data::migration;
use rusty_data::rusqlite::Connection;

use crate::schema;

pub struct ChronosStore {
    conn: Arc<Mutex<Connection>>,
    mode: ConnectionMode,
}

impl ChronosStore {
    pub fn open(path: &Path) -> Result<Self> {
        let config = ConnectionConfig::read_write(path);
        let conn = connection::open(&config)
            .with_context(|| format!("failed to open chronos DB at {}", path.display()))?;
        migration::run(&conn, &schema::migrations())
            .context("failed to run chronos migrations")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            mode: ConnectionMode::ReadWrite,
        })
    }

    pub fn open_readonly(path: &Path) -> Result<Self> {
        let config = ConnectionConfig::read_only(path);
        let conn = connection::open(&config)
            .with_context(|| format!("failed to open chronos DB (readonly) at {}", path.display()))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            mode: ConnectionMode::ReadOnly,
        })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = connection::open_in_memory()?;
        migration::run(&conn, &schema::migrations())
            .context("failed to run chronos migrations (in-memory)")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            mode: ConnectionMode::ReadWrite,
        })
    }

    pub fn connection(&self) -> &Arc<Mutex<Connection>> {
        &self.conn
    }

    pub fn is_readonly(&self) -> bool {
        self.mode == ConnectionMode::ReadOnly
    }

    pub fn default_path() -> Result<std::path::PathBuf> {
        let dir = dirs::home_dir()
            .context("no home directory")?
            .join(".rusty-data");
        Ok(dir.join("billing.db"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_opens() {
        let store = ChronosStore::in_memory().unwrap();
        assert!(!store.is_readonly());
    }

    #[test]
    fn default_path_ends_with_billing_db() {
        let path = ChronosStore::default_path().unwrap();
        assert!(path.ends_with("billing.db"));
        assert!(path.to_string_lossy().contains(".rusty-data"));
    }
}
