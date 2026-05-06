use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use chronos_core::entities::{BillingRate, TimeBlock};

#[derive(Debug, Serialize, Deserialize)]
pub struct TimeSummary {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub total_active_minutes: f64,
    pub total_passive_minutes: f64,
    pub total_compute_only_cost: f64,
    pub total_evidence_minutes: f64,
    pub billable_hours: f64,
    pub total_cost_usd: f64,
    pub by_source: HashMap<String, SourceSummary>,
    pub parallel_session_count: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SourceSummary {
    pub active_minutes: f64,
    pub passive_minutes: f64,
    pub compute_cost: f64,
    pub block_count: usize,
}

pub fn summarize(blocks: &[TimeBlock], rate_usd_hr: f64) -> TimeSummary {
    let mut active = 0.0f64;
    let mut passive = 0.0f64;
    let mut compute_cost = 0.0f64;
    let mut evidence = 0.0f64;
    let mut by_source: HashMap<String, SourceSummary> = HashMap::new();
    let mut parallel_count = 0usize;
    let mut period_start = DateTime::<Utc>::MAX_UTC;
    let mut period_end = DateTime::<Utc>::MIN_UTC;

    for block in blocks {
        if block.start_ts < period_start { period_start = block.start_ts; }
        if block.end_ts > period_end { period_end = block.end_ts; }

        let entry = by_source.entry(block.source.clone()).or_default();
        entry.block_count += 1;

        match block.billing_rate {
            BillingRate::Active => {
                active += block.duration_minutes;
                entry.active_minutes += block.duration_minutes;
                let cost = block.cost_usd.unwrap_or(0.0);
                compute_cost += cost;
                entry.compute_cost += cost;
            }
            BillingRate::Passive => {
                passive += block.duration_minutes;
                entry.passive_minutes += block.duration_minutes;
                let cost = block.cost_usd.unwrap_or(0.0);
                compute_cost += cost;
                entry.compute_cost += cost;
            }
            BillingRate::ComputeOnly => { let cost = block.cost_usd.unwrap_or(0.0); compute_cost += cost; entry.compute_cost += cost; }
            BillingRate::Evidence => { evidence += block.duration_minutes; }
        }

        if block.parallel_index > 0 { parallel_count += 1; }
    }

    let billable_hours = (active / 60.0) + (passive * 0.1 / 60.0);
    let time_cost = billable_hours * rate_usd_hr;
    let total_cost = time_cost + compute_cost;

    TimeSummary {
        period_start, period_end,
        total_active_minutes: active, total_passive_minutes: passive,
        total_compute_only_cost: compute_cost, total_evidence_minutes: evidence,
        billable_hours, total_cost_usd: total_cost,
        by_source, parallel_session_count: parallel_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(source: &str, rate: BillingRate, minutes: f64, cost: Option<f64>) -> TimeBlock {
        TimeBlock {
            id: None, billing_project_id: 1, source: source.into(),
            start_ts: "2026-05-06T10:00:00Z".parse().unwrap(),
            end_ts: "2026-05-06T11:00:00Z".parse().unwrap(),
            duration_minutes: minutes, billing_rate: rate,
            rate_multiplier: rate.multiplier(), cost_usd: cost,
            parallel_index: 0, parallel_total: 1, parallel_label: None,
            source_event_ids: None, computed_at: Utc::now(),
        }
    }

    #[test]
    fn active_only_billing() {
        let blocks = vec![block("claude", BillingRate::Active, 60.0, Some(1.0))];
        let summary = summarize(&blocks, 150.0);
        assert!((summary.total_active_minutes - 60.0).abs() < 0.01);
        assert!((summary.billable_hours - 1.0).abs() < 0.01);
        assert!((summary.total_cost_usd - 151.0).abs() < 0.01);
    }

    #[test]
    fn passive_at_10_percent() {
        let blocks = vec![
            block("claude", BillingRate::Active, 30.0, Some(0.5)),
            block("claude", BillingRate::Passive, 30.0, Some(0.5)),
        ];
        let summary = summarize(&blocks, 100.0);
        assert!((summary.billable_hours - 0.55).abs() < 0.01);
    }

    #[test]
    fn compute_only_adds_cost_not_hours() {
        let blocks = vec![block("claude", BillingRate::ComputeOnly, 5.0, Some(0.25))];
        let summary = summarize(&blocks, 150.0);
        assert!((summary.billable_hours).abs() < 0.01);
        assert!((summary.total_compute_only_cost - 0.25).abs() < 0.01);
    }

    #[test]
    fn by_source_breakdown() {
        let blocks = vec![
            block("claude", BillingRate::Active, 60.0, None),
            block("github", BillingRate::Active, 30.0, None),
        ];
        let summary = summarize(&blocks, 100.0);
        assert_eq!(summary.by_source.len(), 2);
        assert!((summary.by_source["claude"].active_minutes - 60.0).abs() < 0.01);
    }
}
