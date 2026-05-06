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

    pub fn insert_client(&self, client: &crate::entities::Client) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO client (name, contact, rate_usd_hr, created_at, notes) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusty_data::rusqlite::params![
                client.name, client.contact, client.rate_usd_hr,
                client.created_at.to_rfc3339(), client.notes,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_project(&self, project: &crate::entities::BillingProject) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO billing_project (client_id, name, billing_type, rate_override, budget_hours, status, created_at, goals_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusty_data::rusqlite::params![
                project.client_id, project.name, project.billing_type.as_str(),
                project.rate_override, project.budget_hours, project.status.as_str(),
                project.created_at.to_rfc3339(), project.goals_json,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_clients(&self) -> Result<Vec<crate::entities::Client>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, contact, rate_usd_hr, created_at, notes FROM client ORDER BY name"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::entities::Client {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                contact: row.get(2)?,
                rate_usd_hr: row.get(3)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .unwrap_or_default().with_timezone(&chrono::Utc),
                notes: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_projects(&self, client_id: Option<i64>) -> Result<Vec<crate::entities::BillingProject>> {
        let conn = self.conn.lock().unwrap();
        let sql = match client_id {
            Some(_) => "SELECT id, client_id, name, billing_type, rate_override, budget_hours, status, created_at, goals_json FROM billing_project WHERE client_id = ?1 ORDER BY name",
            None => "SELECT id, client_id, name, billing_type, rate_override, budget_hours, status, created_at, goals_json FROM billing_project ORDER BY name",
        };
        let mut stmt = conn.prepare(sql)?;
        let params: Vec<Box<dyn rusty_data::rusqlite::types::ToSql>> = match client_id {
            Some(id) => vec![Box::new(id)],
            None => vec![],
        };
        let param_refs: Vec<&dyn rusty_data::rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let bt_str: String = row.get(3)?;
            let st_str: String = row.get(6)?;
            Ok(crate::entities::BillingProject {
                id: Some(row.get(0)?),
                client_id: row.get(1)?,
                name: row.get(2)?,
                billing_type: crate::entities::BillingType::from_str(&bt_str).unwrap_or(crate::entities::BillingType::Hourly),
                rate_override: row.get(4)?,
                budget_hours: row.get(5)?,
                status: crate::entities::ProjectStatus::from_str(&st_str).unwrap_or(crate::entities::ProjectStatus::Active),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap_or_default().with_timezone(&chrono::Utc),
                goals_json: row.get(8)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn insert_activity_event(&self, event: &crate::entities::ActivityEvent) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO activity_event (source, source_event_id, billing_project_id, event_type, timestamp, end_timestamp, actor, summary, metadata_json, preliminary_project_id, needs_llm_review, ingested_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusty_data::rusqlite::params![
                event.source, event.source_event_id, event.billing_project_id,
                event.event_type, event.timestamp.to_rfc3339(),
                event.end_timestamp.map(|t| t.to_rfc3339()),
                event.actor, event.summary, event.metadata_json,
                event.preliminary_project_id, event.needs_llm_review as i32,
                event.ingested_at.to_rfc3339(),
            ],
        )?;
        if conn.changes() == 0 {
            let id = conn.query_row(
                "SELECT id FROM activity_event WHERE source = ?1 AND source_event_id = ?2",
                rusty_data::rusqlite::params![event.source, event.source_event_id],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(id)
        } else {
            Ok(conn.last_insert_rowid())
        }
    }

    pub fn insert_time_block(&self, block: &crate::entities::TimeBlock) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO time_block (billing_project_id, source, start_ts, end_ts, duration_minutes, billing_rate, rate_multiplier, cost_usd, parallel_index, parallel_total, parallel_label, source_event_ids, computed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusty_data::rusqlite::params![
                block.billing_project_id, block.source,
                block.start_ts.to_rfc3339(), block.end_ts.to_rfc3339(),
                block.duration_minutes, block.billing_rate.as_str(),
                block.rate_multiplier, block.cost_usd,
                block.parallel_index, block.parallel_total,
                block.parallel_label, block.source_event_ids,
                block.computed_at.to_rfc3339(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn upsert_cursor(&self, source: &str, cursor: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO connector_cursor (source, last_cursor, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(source) DO UPDATE SET last_cursor = ?2, updated_at = ?3",
            rusty_data::rusqlite::params![source, cursor, now],
        )?;
        Ok(())
    }

    pub fn get_cursor(&self, source: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT last_cursor FROM connector_cursor WHERE source = ?1",
            rusty_data::rusqlite::params![source],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(cursor) => Ok(Some(cursor)),
            Err(rusty_data::rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
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

    #[test]
    fn insert_and_query_client() {
        let store = ChronosStore::in_memory().unwrap();
        let conn = store.connection().lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO client (name, rate_usd_hr, created_at) VALUES (?1, ?2, ?3)",
            rusty_data::rusqlite::params!["Acme Corp", 150.0, now],
        ).unwrap();
        let name: String = conn.query_row(
            "SELECT name FROM client WHERE id = 1", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(name, "Acme Corp");
    }

    #[test]
    fn insert_project_with_fk() {
        let store = ChronosStore::in_memory().unwrap();
        let conn = store.connection().lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO client (name, rate_usd_hr, created_at) VALUES (?1, ?2, ?3)",
            rusty_data::rusqlite::params!["Acme", 100.0, now],
        ).unwrap();
        conn.execute(
            "INSERT INTO billing_project (client_id, name, billing_type, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusty_data::rusqlite::params![1, "Website Redesign", "hourly", now],
        ).unwrap();
        let project_name: String = conn.query_row(
            "SELECT name FROM billing_project WHERE client_id = 1", [], |row| row.get(0),
        ).unwrap();
        assert_eq!(project_name, "Website Redesign");
    }

    #[test]
    fn fk_constraint_rejects_orphan_project() {
        let store = ChronosStore::in_memory().unwrap();
        let conn = store.connection().lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let result = conn.execute(
            "INSERT INTO billing_project (client_id, name, billing_type, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusty_data::rusqlite::params![999, "Orphan", "hourly", now],
        );
        assert!(result.is_err());
    }

    #[test]
    fn typed_client_roundtrip() {
        use crate::entities::*;
        let store = ChronosStore::in_memory().unwrap();
        let client = Client {
            id: None, name: "Acme Corp".into(), contact: Some("alice@acme.com".into()),
            rate_usd_hr: 150.0, created_at: chrono::Utc::now(), notes: None,
        };
        let id = store.insert_client(&client).unwrap();
        assert_eq!(id, 1);
        let clients = store.list_clients().unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].name, "Acme Corp");
        assert_eq!(clients[0].rate_usd_hr, 150.0);
    }

    #[test]
    fn duplicate_event_returns_existing_id() {
        use crate::entities::*;
        let store = ChronosStore::in_memory().unwrap();
        let conn = store.connection().lock().unwrap();
        let now = chrono::Utc::now();
        conn.execute(
            "INSERT INTO client (name, rate_usd_hr, created_at) VALUES (?1, ?2, ?3)",
            rusty_data::rusqlite::params!["Test", 100.0, now.to_rfc3339()],
        ).unwrap();
        conn.execute(
            "INSERT INTO billing_project (client_id, name, billing_type, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusty_data::rusqlite::params![1, "Proj", "hourly", now.to_rfc3339()],
        ).unwrap();
        drop(conn);

        let event = ActivityEvent {
            id: None, source: "claude".into(), source_event_id: "s1".into(),
            billing_project_id: Some(1), event_type: "session".into(),
            timestamp: now, end_timestamp: None, actor: None, summary: None,
            metadata_json: None, preliminary_project_id: None,
            needs_llm_review: false, ingested_at: now,
        };
        let first_id = store.insert_activity_event(&event).unwrap();
        let second_id = store.insert_activity_event(&event).unwrap();
        assert_eq!(first_id, second_id);
    }

    #[test]
    fn cursor_upsert_and_get() {
        let store = ChronosStore::in_memory().unwrap();
        assert!(store.get_cursor("claude").unwrap().is_none());
        store.upsert_cursor("claude", "2026-05-01T00:00:00Z").unwrap();
        assert_eq!(store.get_cursor("claude").unwrap().unwrap(), "2026-05-01T00:00:00Z");
        store.upsert_cursor("claude", "2026-05-06T00:00:00Z").unwrap();
        assert_eq!(store.get_cursor("claude").unwrap().unwrap(), "2026-05-06T00:00:00Z");
    }
}
