use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub id: Option<i64>,
    pub name: String,
    pub contact: Option<String>,
    pub rate_usd_hr: f64,
    pub created_at: DateTime<Utc>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingType {
    Hourly,
    Fixed,
    ComputeOnly,
}

impl BillingType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::Fixed => "fixed",
            Self::ComputeOnly => "compute_only",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "hourly" => Some(Self::Hourly),
            "fixed" => Some(Self::Fixed),
            "compute_only" => Some(Self::ComputeOnly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active, Paused, Closed,
}

impl ProjectStatus {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Active => "active", Self::Paused => "paused", Self::Closed => "closed" }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s { "active" => Some(Self::Active), "paused" => Some(Self::Paused), "closed" => Some(Self::Closed), _ => None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingProject {
    pub id: Option<i64>,
    pub client_id: i64,
    pub name: String,
    pub billing_type: BillingType,
    pub rate_override: Option<f64>,
    pub budget_hours: Option<f64>,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
    pub goals_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleType { Channel, Label, Keyword, Path, Llm }

impl RuleType {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Channel => "channel", Self::Label => "label", Self::Keyword => "keyword", Self::Path => "path", Self::Llm => "llm" }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s { "channel" => Some(Self::Channel), "label" => Some(Self::Label), "keyword" => Some(Self::Keyword), "path" => Some(Self::Path), "llm" => Some(Self::Llm), _ => None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionRule {
    pub id: Option<i64>,
    pub billing_project_id: i64,
    pub source: String,
    pub rule_type: RuleType,
    pub pattern: String,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: Option<i64>,
    pub source: String,
    pub source_event_id: String,
    pub billing_project_id: Option<i64>,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub end_timestamp: Option<DateTime<Utc>>,
    pub actor: Option<String>,
    pub summary: Option<String>,
    pub metadata_json: Option<String>,
    pub preliminary_project_id: Option<i64>,
    pub needs_llm_review: bool,
    pub ingested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingRate { Active, Passive, ComputeOnly, Evidence }

impl BillingRate {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Active => "active", Self::Passive => "passive", Self::ComputeOnly => "compute_only", Self::Evidence => "evidence" }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s { "active" => Some(Self::Active), "passive" => Some(Self::Passive), "compute_only" => Some(Self::ComputeOnly), "evidence" => Some(Self::Evidence), _ => None }
    }
    pub fn multiplier(&self) -> f64 {
        match self { Self::Active => 1.0, Self::Passive => 0.1, Self::ComputeOnly => 0.0, Self::Evidence => 0.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeBlock {
    pub id: Option<i64>,
    pub billing_project_id: i64,
    pub source: String,
    pub start_ts: DateTime<Utc>,
    pub end_ts: DateTime<Utc>,
    pub duration_minutes: f64,
    pub billing_rate: BillingRate,
    pub rate_multiplier: f64,
    pub cost_usd: Option<f64>,
    pub parallel_index: i32,
    pub parallel_total: i32,
    pub parallel_label: Option<String>,
    pub source_event_ids: Option<String>,
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence { High, Medium, Low }

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self { Self::High => "high", Self::Medium => "medium", Self::Low => "low" }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: Option<i64>,
    pub billing_project_id: i64,
    pub summary: String,
    pub alternatives: Option<String>,
    pub rationale: Option<String>,
    pub confidence: Option<Confidence>,
    pub source: String,
    pub source_event_id: Option<String>,
    pub extracted_at: DateTime<Utc>,
    pub content_hash: String,
    pub consequence: Option<String>,
    pub consequence_status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus { Todo, InProgress, Done, Blocked }

impl WorkItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Todo => "todo", Self::InProgress => "in_progress", Self::Done => "done", Self::Blocked => "blocked" }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s { "todo" => Some(Self::Todo), "in_progress" => Some(Self::InProgress), "done" => Some(Self::Done), "blocked" => Some(Self::Blocked), _ => None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: Option<i64>,
    pub billing_project_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub status: WorkItemStatus,
    pub completion_pct: f64,
    pub source: Option<String>,
    pub source_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorCursor {
    pub source: String,
    pub last_cursor: String,
    pub updated_at: DateTime<Utc>,
}
