use chrono::{DateTime, Utc};
use crate::{ConnectorStatus, RawEvent, SourceConnector};

pub struct GmailConnector {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    client: reqwest::Client,
}

impl GmailConnector {
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
            .ok_or_else(|| anyhow::anyhow!("no access_token"))
    }

    async fn fetch_async(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let token = self.get_access_token().await?;
        let after_epoch = since.timestamp();
        let query = format!("after:{after_epoch}");

        let resp = self.client
            .get("https://gmail.googleapis.com/gmail/v1/users/me/messages")
            .bearer_auth(&token)
            .query(&[("q", &query), ("maxResults", &"100".to_string())])
            .send().await?;

        if !resp.status().is_success() { return Ok(vec![]); }

        let body: serde_json::Value = resp.json().await?;
        let messages = body["messages"].as_array().cloned().unwrap_or_default();

        let mut events = vec![];
        for msg_ref in messages.iter().take(50) {
            let msg_id = match msg_ref["id"].as_str() { Some(id) => id, None => continue };

            let detail_resp = self.client
                .get(format!("https://gmail.googleapis.com/gmail/v1/users/me/messages/{msg_id}"))
                .bearer_auth(&token)
                .query(&[("format", "metadata"), ("metadataHeaders", "Subject"), ("metadataHeaders", "From")])
                .send().await?;

            if !detail_resp.status().is_success() { continue; }
            let detail: serde_json::Value = detail_resp.json().await?;

            let internal_date = detail["internalDate"].as_str()
                .and_then(|d| d.parse::<i64>().ok()).unwrap_or(0);
            let ts = DateTime::from_timestamp_millis(internal_date).unwrap_or(since);

            let headers = detail["payload"]["headers"].as_array();
            let subject = headers.and_then(|h| h.iter().find(|x| x["name"] == "Subject"))
                .and_then(|h| h["value"].as_str()).unwrap_or("(no subject)");
            let from = headers.and_then(|h| h.iter().find(|x| x["name"] == "From"))
                .and_then(|h| h["value"].as_str()).map(String::from);

            events.push(RawEvent {
                source: "gmail".into(),
                source_event_id: format!("gmail:{msg_id}"),
                event_type: "gmail_message".into(),
                timestamp: ts,
                end_timestamp: None,
                actor: from,
                summary: Some(subject.to_string()),
                metadata_json: None,
            });
        }
        Ok(events)
    }
}

impl SourceConnector for GmailConnector {
    fn source_id(&self) -> &str { "gmail" }
    fn fetch_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| tokio::runtime::Runtime::new().unwrap().handle().clone());
        rt.block_on(async { self.fetch_async(since).await })
    }
    fn health_check(&self) -> ConnectorStatus {
        ConnectorStatus {
            source: "gmail".into(),
            healthy: !self.refresh_token.is_empty(),
            message: if self.refresh_token.is_empty() { "not configured".into() } else { "configured".into() },
            last_fetch: None,
        }
    }
}
