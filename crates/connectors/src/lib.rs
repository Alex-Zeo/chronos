pub mod claude;
pub mod stella;
pub mod github;
pub mod gdrive;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub source: String,
    pub source_event_id: String,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub end_timestamp: Option<DateTime<Utc>>,
    pub actor: Option<String>,
    pub summary: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorStatus {
    pub source: String,
    pub healthy: bool,
    pub message: String,
    pub last_fetch: Option<DateTime<Utc>>,
}

pub trait SourceConnector {
    fn source_id(&self) -> &str;
    fn fetch_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>>;
    fn health_check(&self) -> ConnectorStatus;
}
