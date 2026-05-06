use chrono::{DateTime, Utc};
use crate::{ConnectorStatus, RawEvent, SourceConnector};

pub struct GDriveConnector {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    client: reqwest::Client,
}

impl GDriveConnector {
    pub fn new(client_id: String, client_secret: String, refresh_token: String) -> Self {
        Self { client_id, client_secret, refresh_token, client: reqwest::Client::new() }
    }

    async fn get_access_token(&self) -> anyhow::Result<String> {
        let resp = self.client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("refresh_token", &self.refresh_token),
                ("grant_type", &"refresh_token".to_string()),
            ]).send().await?;
        let body: serde_json::Value = resp.json().await?;
        body["access_token"].as_str().map(String::from)
            .ok_or_else(|| anyhow::anyhow!("no access_token in response"))
    }

    async fn fetch_async(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let token = self.get_access_token().await?;
        let filter = format!("time >= \"{}\"", since.to_rfc3339());

        let resp = self.client
            .post("https://driveactivity.googleapis.com/v2/activity:query")
            .bearer_auth(&token)
            .json(&serde_json::json!({ "filter": filter, "pageSize": 100 }))
            .send().await?;

        if !resp.status().is_success() {
            tracing::warn!("Drive Activity API returned {}", resp.status());
            return Ok(vec![]);
        }

        let body: serde_json::Value = resp.json().await?;
        let activities = body["activities"].as_array().cloned().unwrap_or_default();

        let events = activities.into_iter().filter_map(|a| {
            let ts_str = a["timestamp"].as_str()?;
            let ts = ts_str.parse::<DateTime<Utc>>().ok()?;

            let action_detail = &a["primaryActionDetail"];
            let event_type = if action_detail.get("edit").is_some() { "gdrive_edit" }
                else if action_detail.get("comment").is_some() { "gdrive_comment" }
                else if action_detail.get("rename").is_some() { "gdrive_rename" }
                else { "gdrive_activity" };

            let target_name = a["targets"].as_array()
                .and_then(|t| t.first())
                .and_then(|t| t["driveItem"]["title"].as_str())
                .unwrap_or("unknown");

            let actor = a["actors"].as_array()
                .and_then(|actors| actors.first())
                .and_then(|actor| actor["user"]["knownUser"]["personName"].as_str())
                .map(String::from);

            Some(RawEvent {
                source: "gdrive".into(),
                source_event_id: format!("gdrive:{}:{}", target_name, ts.timestamp()),
                event_type: event_type.into(),
                timestamp: ts,
                end_timestamp: None,
                actor,
                summary: Some(format!("{}: {}", event_type, target_name)),
                metadata_json: Some(a.to_string()),
            })
        }).collect();

        Ok(events)
    }
}

impl SourceConnector for GDriveConnector {
    fn source_id(&self) -> &str { "gdrive" }

    fn fetch_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| tokio::runtime::Runtime::new().unwrap().handle().clone());
        rt.block_on(async { self.fetch_async(since).await })
    }

    fn health_check(&self) -> ConnectorStatus {
        ConnectorStatus {
            source: "gdrive".into(),
            healthy: !self.refresh_token.is_empty(),
            message: if self.refresh_token.is_empty() { "Google Drive credentials not configured".into() }
            else { "credentials configured".into() },
            last_fetch: None,
        }
    }
}
