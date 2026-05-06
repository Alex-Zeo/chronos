use chrono::{DateTime, Utc};
use rusty_data::bloomnet::BloomnetStore;

use crate::{ConnectorStatus, RawEvent, SourceConnector};

pub struct ClaudeConnector {
    bloomnet: BloomnetStore,
}

impl ClaudeConnector {
    pub fn new(bloomnet: BloomnetStore) -> Self {
        Self { bloomnet }
    }
}

impl SourceConnector for ClaudeConnector {
    fn source_id(&self) -> &str {
        "claude"
    }

    fn fetch_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let conn = self.bloomnet.connection().lock().unwrap();
        let since_str = since.to_rfc3339();

        let mut stmt = conn.prepare(
            "SELECT id, project_name, session_type, start_time, end_time, total_cost_usd, model, parent_id, account_name
             FROM sessions
             WHERE start_time >= ?1
             ORDER BY start_time"
        )?;

        let events: Vec<RawEvent> = stmt
            .query_map(rusty_data::rusqlite::params![since_str], |row| {
                let session_id: String = row.get(0)?;
                let project_name: String = row.get(1)?;
                let session_type: Option<String> = row.get(2)?;
                let start_time: String = row.get(3)?;
                let end_time: String = row.get(4)?;
                let cost_usd: f64 = row.get(5)?;
                let model: Option<String> = row.get(6)?;
                let parent_id: Option<String> = row.get(7)?;
                let account_name: Option<String> = row.get(8)?;

                let metadata = serde_json::json!({
                    "session_type": session_type,
                    "project_name": project_name,
                    "model": model,
                    "parent_id": parent_id,
                    "account_name": account_name,
                    "cost_usd": cost_usd,
                });

                let event_type = match session_type.as_deref() {
                    Some("User") => "claude_session_user",
                    Some("Subagent") => "claude_session_subagent",
                    Some("Hook") => "claude_session_hook",
                    Some("Cron") => "claude_session_cron",
                    _ => "claude_session_user",
                };

                let start = chrono::DateTime::parse_from_rfc3339(&start_time)
                    .unwrap_or_default().with_timezone(&chrono::Utc);
                let end = chrono::DateTime::parse_from_rfc3339(&end_time)
                    .unwrap_or_default().with_timezone(&chrono::Utc);

                Ok(RawEvent {
                    source: "claude".into(),
                    source_event_id: session_id,
                    event_type: event_type.into(),
                    timestamp: start,
                    end_timestamp: Some(end),
                    actor: account_name,
                    summary: Some(format!("{} on {}", event_type, project_name)),
                    metadata_json: Some(metadata.to_string()),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(events)
    }

    fn health_check(&self) -> ConnectorStatus {
        let healthy = self.bloomnet.connection().lock().is_ok();
        ConnectorStatus {
            source: "claude".into(),
            healthy,
            message: if healthy { "bloomnet.db accessible".into() } else { "cannot lock connection".into() },
            last_fetch: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_bloomnet(store: &BloomnetStore) {
        let conn = store.connection().lock().unwrap();
        conn.execute_batch(
            "INSERT INTO sessions (id, user_id, project_id, project_name, start_time, end_time, total_cost_usd, total_duration_ms, session_type, created_at, total_input_tokens, total_output_tokens, total_cache_creation_tokens, total_cache_read_tokens, task_count, turn_count, tool_call_count)
             VALUES
               ('s1', 'u1', 'p1', 'chronos', '2026-05-06T10:00:00Z', '2026-05-06T10:30:00Z', 0.50, 1800000, 'User', '2026-05-06T10:00:00Z', 1000, 2000, 0, 0, 1, 5, 10),
               ('s2', 'u1', 'p1', 'chronos', '2026-05-06T11:00:00Z', '2026-05-06T11:05:00Z', 0.02, 300000, 'Hook', '2026-05-06T11:00:00Z', 100, 200, 0, 0, 0, 1, 1),
               ('s3', 'u1', 'p1', 'chronos', '2026-05-06T12:00:00Z', '2026-05-06T12:45:00Z', 1.20, 2700000, 'User', '2026-05-06T12:00:00Z', 5000, 8000, 0, 0, 2, 12, 25);"
        ).unwrap();
    }

    #[test]
    fn fetch_returns_sessions_after_since() {
        let bloomnet = BloomnetStore::in_memory().unwrap();
        seed_bloomnet(&bloomnet);
        let connector = ClaudeConnector::new(bloomnet);
        let since = "2026-05-06T10:30:00Z".parse::<DateTime<Utc>>().unwrap();
        let events = connector.fetch_since(since).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].source_event_id, "s2");
        assert_eq!(events[0].event_type, "claude_session_hook");
        assert_eq!(events[1].source_event_id, "s3");
    }

    #[test]
    fn fetch_all_sessions() {
        let bloomnet = BloomnetStore::in_memory().unwrap();
        seed_bloomnet(&bloomnet);
        let connector = ClaudeConnector::new(bloomnet);
        let since = "2000-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let events = connector.fetch_since(since).unwrap();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn health_check_reports_healthy() {
        let bloomnet = BloomnetStore::in_memory().unwrap();
        let connector = ClaudeConnector::new(bloomnet);
        assert!(connector.health_check().healthy);
    }

    #[test]
    fn metadata_contains_cost() {
        let bloomnet = BloomnetStore::in_memory().unwrap();
        seed_bloomnet(&bloomnet);
        let connector = ClaudeConnector::new(bloomnet);
        let since = "2000-01-01T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let events = connector.fetch_since(since).unwrap();
        let meta: serde_json::Value = serde_json::from_str(events[0].metadata_json.as_ref().unwrap()).unwrap();
        assert_eq!(meta["cost_usd"], 0.50);
        assert_eq!(meta["session_type"], "User");
    }
}
