use chrono::{DateTime, Duration, Utc};

use crate::entities::{BillingRate, TimeBlock};

const ACTIVE_THRESHOLD_MINUTES: i64 = 30;

pub struct ClaudeSession {
    pub session_id: String,
    pub project_id: i64,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub session_type: String,
    pub cost_usd: f64,
}

pub fn attribute_claude_session(session: &ClaudeSession, rate_usd_hr: f64) -> Vec<TimeBlock> {
    let now = Utc::now();
    let is_compute_only = matches!(
        session.session_type.as_str(),
        "Hook" | "Cron" | "Subagent"
    );

    if is_compute_only {
        let duration = (session.end - session.start).num_seconds() as f64 / 60.0;
        return vec![TimeBlock {
            id: None,
            billing_project_id: session.project_id,
            source: "claude".into(),
            start_ts: session.start,
            end_ts: session.end,
            duration_minutes: duration,
            billing_rate: BillingRate::ComputeOnly,
            rate_multiplier: 0.0,
            cost_usd: Some(session.cost_usd),
            parallel_index: 0,
            parallel_total: 1,
            parallel_label: None,
            source_event_ids: Some(session.session_id.clone()),
            computed_at: now,
        }];
    }

    let total_minutes = (session.end - session.start).num_seconds() as f64 / 60.0;
    if total_minutes <= 0.0 {
        return vec![];
    }

    let active_minutes = total_minutes.min(ACTIVE_THRESHOLD_MINUTES as f64);
    let passive_minutes = (total_minutes - active_minutes).max(0.0);

    let mut blocks = vec![];

    let active_end = session.start + Duration::minutes(active_minutes.ceil() as i64);
    blocks.push(TimeBlock {
        id: None,
        billing_project_id: session.project_id,
        source: "claude".into(),
        start_ts: session.start,
        end_ts: active_end.min(session.end),
        duration_minutes: active_minutes,
        billing_rate: BillingRate::Active,
        rate_multiplier: 1.0,
        cost_usd: Some(session.cost_usd * (active_minutes / total_minutes)),
        parallel_index: 0,
        parallel_total: 1,
        parallel_label: None,
        source_event_ids: Some(session.session_id.clone()),
        computed_at: now,
    });

    if passive_minutes > 0.0 {
        blocks.push(TimeBlock {
            id: None,
            billing_project_id: session.project_id,
            source: "claude".into(),
            start_ts: active_end,
            end_ts: session.end,
            duration_minutes: passive_minutes,
            billing_rate: BillingRate::Passive,
            rate_multiplier: 0.1,
            cost_usd: Some(session.cost_usd * (passive_minutes / total_minutes)),
            parallel_index: 0,
            parallel_total: 1,
            parallel_label: None,
            source_event_ids: Some(session.session_id.clone()),
            computed_at: now,
        });
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(start: &str, end: &str, session_type: &str, cost: f64) -> ClaudeSession {
        ClaudeSession {
            session_id: "s1".into(),
            project_id: 1,
            start: start.parse().unwrap(),
            end: end.parse().unwrap(),
            session_type: session_type.into(),
            cost_usd: cost,
        }
    }

    #[test]
    fn short_session_is_fully_active() {
        let s = session("2026-05-06T10:00:00Z", "2026-05-06T10:20:00Z", "User", 0.50);
        let blocks = attribute_claude_session(&s, 150.0);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].billing_rate, BillingRate::Active);
        assert!((blocks[0].duration_minutes - 20.0).abs() < 0.01);
    }

    #[test]
    fn long_session_splits_active_passive() {
        let s = session("2026-05-06T10:00:00Z", "2026-05-06T11:00:00Z", "User", 1.00);
        let blocks = attribute_claude_session(&s, 150.0);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].billing_rate, BillingRate::Active);
        assert!((blocks[0].duration_minutes - 30.0).abs() < 0.01);
        assert_eq!(blocks[1].billing_rate, BillingRate::Passive);
        assert!((blocks[1].duration_minutes - 30.0).abs() < 0.01);
        assert_eq!(blocks[1].rate_multiplier, 0.1);
    }

    #[test]
    fn hook_session_is_compute_only() {
        let s = session("2026-05-06T10:00:00Z", "2026-05-06T10:01:00Z", "Hook", 0.01);
        let blocks = attribute_claude_session(&s, 150.0);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].billing_rate, BillingRate::ComputeOnly);
        assert_eq!(blocks[0].cost_usd, Some(0.01));
    }

    #[test]
    fn subagent_is_compute_only() {
        let s = session("2026-05-06T10:00:00Z", "2026-05-06T10:10:00Z", "Subagent", 0.30);
        let blocks = attribute_claude_session(&s, 150.0);
        assert_eq!(blocks[0].billing_rate, BillingRate::ComputeOnly);
    }

    #[test]
    fn cron_is_compute_only() {
        let s = session("2026-05-06T10:00:00Z", "2026-05-06T10:02:00Z", "Cron", 0.05);
        let blocks = attribute_claude_session(&s, 150.0);
        assert_eq!(blocks[0].billing_rate, BillingRate::ComputeOnly);
    }

    #[test]
    fn exactly_30min_session_is_fully_active() {
        let s = session("2026-05-06T10:00:00Z", "2026-05-06T10:30:00Z", "User", 0.80);
        let blocks = attribute_claude_session(&s, 150.0);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].billing_rate, BillingRate::Active);
    }

    #[test]
    fn cost_splits_proportionally() {
        let s = session("2026-05-06T10:00:00Z", "2026-05-06T11:00:00Z", "User", 2.00);
        let blocks = attribute_claude_session(&s, 150.0);
        let total_cost: f64 = blocks.iter().filter_map(|b| b.cost_usd).sum();
        assert!((total_cost - 2.00).abs() < 0.01);
    }
}
