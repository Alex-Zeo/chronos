use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::{ConnectorStatus, RawEvent, SourceConnector};

pub struct StellaConnector {
    vault_db_path: PathBuf,
}

impl StellaConnector {
    pub fn new(vault_db_path: impl Into<PathBuf>) -> Self {
        Self { vault_db_path: vault_db_path.into() }
    }

    pub fn default() -> Self {
        let path = dirs::home_dir().unwrap_or_default().join(".rusty-data").join("vault.db");
        Self::new(path)
    }
}

impl SourceConnector for StellaConnector {
    fn source_id(&self) -> &str { "stella" }

    fn fetch_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        if !self.vault_db_path.exists() {
            tracing::warn!("vault.db not found at {}", self.vault_db_path.display());
            return Ok(vec![]);
        }

        let conn = rusty_data::rusqlite::Connection::open_with_flags(
            &self.vault_db_path,
            rusty_data::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;

        let since_str = since.to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT iri, title, dimension, modified_at, created_at
             FROM frames WHERE modified_at >= ?1 ORDER BY modified_at"
        )?;

        let events = stmt.query_map(rusty_data::rusqlite::params![since_str], |row| {
            let iri: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let dimension: Option<String> = row.get(2)?;
            let modified: String = row.get(3)?;
            let created: Option<String> = row.get(4)?;

            let is_create = created.as_deref() == Some(&modified);
            let event_type = if is_create { "vault_frame_created" } else { "vault_frame_updated" };

            let ts = chrono::DateTime::parse_from_rfc3339(&modified)
                .unwrap_or_default().with_timezone(&Utc);

            let metadata = serde_json::json!({ "iri": iri, "dimension": dimension, "title": title });

            Ok(RawEvent {
                source: "stella".into(),
                source_event_id: format!("vault:{iri}:{}", modified),
                event_type: event_type.into(),
                timestamp: ts,
                end_timestamp: None,
                actor: None,
                summary: title.map(|t| format!("{event_type}: {t}")),
                metadata_json: Some(metadata.to_string()),
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(events)
    }

    fn health_check(&self) -> ConnectorStatus {
        ConnectorStatus {
            source: "stella".into(),
            healthy: self.vault_db_path.exists(),
            message: if self.vault_db_path.exists() { "vault.db found".into() }
            else { format!("vault.db not found at {}", self.vault_db_path.display()) },
            last_fetch: None,
        }
    }
}
