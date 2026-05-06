use chrono::{DateTime, Utc};
use crate::{ConnectorStatus, RawEvent, SourceConnector};

pub struct SlackConnector {
    bot_token: String,
    channels: Vec<String>,
    client: reqwest::Client,
}

impl SlackConnector {
    pub fn new(bot_token: String, channels: Vec<String>) -> Self {
        Self { bot_token, channels, client: reqwest::Client::new() }
    }

    async fn fetch_async(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let mut all_events = vec![];
        let oldest = since.timestamp().to_string();

        for channel in &self.channels {
            let resp = self.client
                .get("https://slack.com/api/conversations.history")
                .bearer_auth(&self.bot_token)
                .query(&[("channel", channel.as_str()), ("oldest", &oldest), ("limit", "200")])
                .send().await?;

            let body: serde_json::Value = resp.json().await?;
            if body["ok"].as_bool() != Some(true) {
                tracing::warn!("Slack API error for {channel}: {}", body["error"]);
                continue;
            }

            let messages = body["messages"].as_array().cloned().unwrap_or_default();
            for msg in messages {
                let ts_str = msg["ts"].as_str().unwrap_or("0");
                let ts_f: f64 = ts_str.parse().unwrap_or(0.0);
                let ts = DateTime::from_timestamp(ts_f as i64, 0).unwrap_or(since);
                let user = msg["user"].as_str().map(String::from);
                let text = msg["text"].as_str().unwrap_or("").to_string();

                all_events.push(RawEvent {
                    source: "slack".into(),
                    source_event_id: format!("slack:{channel}:{ts_str}"),
                    event_type: "slack_message".into(),
                    timestamp: ts,
                    end_timestamp: None,
                    actor: user,
                    summary: Some(if text.len() > 100 { format!("{}...", &text[..97]) } else { text }),
                    metadata_json: Some(serde_json::json!({"channel": channel}).to_string()),
                });
            }
        }
        Ok(all_events)
    }
}

impl SourceConnector for SlackConnector {
    fn source_id(&self) -> &str { "slack" }
    fn fetch_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| tokio::runtime::Runtime::new().unwrap().handle().clone());
        rt.block_on(async { self.fetch_async(since).await })
    }
    fn health_check(&self) -> ConnectorStatus {
        ConnectorStatus {
            source: "slack".into(),
            healthy: !self.bot_token.is_empty(),
            message: if self.bot_token.is_empty() { "SLACK_BOT_TOKEN not set".into() }
            else { format!("{} channels", self.channels.len()) },
            last_fetch: None,
        }
    }
}
