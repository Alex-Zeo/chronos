use chrono::{DateTime, Duration, Utc};

use crate::entities::{BillingRate, TimeBlock};

const CONTINUATION_MINUTES: i64 = 30;
const COMMIT_CLUSTER_MINUTES: f64 = 5.0;

pub struct PointEvent {
    pub timestamp: DateTime<Utc>,
    pub source_event_id: String,
}

pub fn build_continuation_blocks(
    events: &[PointEvent],
    project_id: i64,
    source: &str,
) -> Vec<TimeBlock> {
    if events.is_empty() {
        return vec![];
    }

    let mut sorted: Vec<&PointEvent> = events.iter().collect();
    sorted.sort_by_key(|e| e.timestamp);

    let now = Utc::now();
    let continuation = Duration::minutes(CONTINUATION_MINUTES);
    let mut blocks: Vec<TimeBlock> = vec![];
    let mut block_start = sorted[0].timestamp;
    let mut block_last_touch = sorted[0].timestamp;
    let mut event_ids: Vec<String> = vec![sorted[0].source_event_id.clone()];

    for event in &sorted[1..] {
        let gap = event.timestamp - block_last_touch;
        if gap <= continuation {
            block_last_touch = event.timestamp;
            event_ids.push(event.source_event_id.clone());
        } else {
            let end = block_last_touch + continuation;
            let duration = (end - block_start).num_seconds() as f64 / 60.0;
            blocks.push(TimeBlock {
                id: None,
                billing_project_id: project_id,
                source: source.into(),
                start_ts: block_start,
                end_ts: end,
                duration_minutes: if source == "github" { duration.max(COMMIT_CLUSTER_MINUTES) } else { duration },
                billing_rate: BillingRate::Active,
                rate_multiplier: 1.0,
                cost_usd: None,
                parallel_index: 0,
                parallel_total: 1,
                parallel_label: None,
                source_event_ids: Some(event_ids.join(",")),
                computed_at: now,
            });
            block_start = event.timestamp;
            block_last_touch = event.timestamp;
            event_ids = vec![event.source_event_id.clone()];
        }
    }

    let end = block_last_touch + continuation;
    let duration = (end - block_start).num_seconds() as f64 / 60.0;
    blocks.push(TimeBlock {
        id: None,
        billing_project_id: project_id,
        source: source.into(),
        start_ts: block_start,
        end_ts: end,
        duration_minutes: duration.max(COMMIT_CLUSTER_MINUTES),
        billing_rate: BillingRate::Active,
        rate_multiplier: 1.0,
        cost_usd: None,
        parallel_index: 0,
        parallel_total: 1,
        parallel_label: None,
        source_event_ids: Some(event_ids.join(",")),
        computed_at: now,
    });

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evt(ts: &str, id: &str) -> PointEvent {
        PointEvent { timestamp: ts.parse().unwrap(), source_event_id: id.into() }
    }

    #[test]
    fn single_event_gets_30min_extension() {
        let events = vec![evt("2026-05-06T14:00:00Z", "e1")];
        let blocks = build_continuation_blocks(&events, 1, "gdrive");
        assert_eq!(blocks.len(), 1);
        assert!((blocks[0].duration_minutes - 30.0).abs() < 0.01);
    }

    #[test]
    fn events_within_30min_merge() {
        let events = vec![
            evt("2026-05-06T14:00:00Z", "e1"),
            evt("2026-05-06T14:15:00Z", "e2"),
            evt("2026-05-06T14:20:00Z", "e3"),
        ];
        let blocks = build_continuation_blocks(&events, 1, "gdrive");
        assert_eq!(blocks.len(), 1);
        assert!((blocks[0].duration_minutes - 50.0).abs() < 0.01);
    }

    #[test]
    fn gap_over_30min_creates_new_block() {
        let events = vec![
            evt("2026-05-06T14:00:00Z", "e1"),
            evt("2026-05-06T15:00:00Z", "e2"),
        ];
        let blocks = build_continuation_blocks(&events, 1, "gdrive");
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn empty_events_returns_empty() {
        let blocks = build_continuation_blocks(&[], 1, "gdrive");
        assert!(blocks.is_empty());
    }

    #[test]
    fn commit_cluster_gets_minimum_5min() {
        let events = vec![
            evt("2026-05-06T14:00:00Z", "c1"),
            evt("2026-05-06T14:00:30Z", "c2"),
        ];
        let blocks = build_continuation_blocks(&events, 1, "github");
        assert!(blocks[0].duration_minutes >= COMMIT_CLUSTER_MINUTES);
    }

    #[test]
    fn non_github_source_gets_raw_duration() {
        let events = vec![
            evt("2026-05-06T14:00:00Z", "e1"),
            evt("2026-05-06T14:00:30Z", "e2"),
        ];
        let blocks = build_continuation_blocks(&events, 1, "gdrive");
        // Raw: (14:00:30 + 30min) - 14:00:00 = 30.5 min, no 5-min floor applied
        assert!((blocks[0].duration_minutes - 30.5).abs() < 0.01);
    }

    #[test]
    fn event_ids_tracked() {
        let events = vec![evt("2026-05-06T14:00:00Z", "e1"), evt("2026-05-06T14:10:00Z", "e2")];
        let blocks = build_continuation_blocks(&events, 1, "gdrive");
        assert_eq!(blocks[0].source_event_ids.as_deref(), Some("e1,e2"));
    }

    #[test]
    fn unsorted_events_handled() {
        let events = vec![evt("2026-05-06T14:20:00Z", "e2"), evt("2026-05-06T14:00:00Z", "e1")];
        let blocks = build_continuation_blocks(&events, 1, "gdrive");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_ts, "2026-05-06T14:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }
}
