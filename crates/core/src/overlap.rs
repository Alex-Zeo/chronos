use crate::entities::{BillingRate, TimeBlock};

pub fn detect_parallel_claude_sessions(blocks: &mut [TimeBlock]) {
    let claude_indices: Vec<usize> = blocks.iter().enumerate()
        .filter(|(_, b)| b.source == "claude" && b.billing_rate != BillingRate::ComputeOnly)
        .map(|(i, _)| i).collect();

    let n = claude_indices.len();
    if n <= 1 { return; }

    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            parent[i] = find(parent, parent[i]);
        }
        parent[i]
    }

    for i in 0..n {
        for j in (i + 1)..n {
            let a = &blocks[claude_indices[i]];
            let b = &blocks[claude_indices[j]];
            if a.start_ts < b.end_ts && b.start_ts < a.end_ts {
                let ri = find(&mut parent, i);
                let rj = find(&mut parent, j);
                if ri != rj { parent[rj] = ri; }
            }
        }
    }

    let mut components: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        components.entry(root).or_default().push(i);
    }

    for members in components.values() {
        if members.len() <= 1 { continue; }
        let total = members.len();
        let mut sorted = members.clone();
        sorted.sort_by_key(|&m| blocks[claude_indices[m]].start_ts);
        for (rank, &m) in sorted.iter().enumerate() {
            let idx = claude_indices[m];
            blocks[idx].parallel_total = total as i32;
            blocks[idx].parallel_index = rank as i32;
            blocks[idx].parallel_label = Some(format!(
                "Session {} ({} of {} concurrent)",
                (b'A' + rank as u8) as char, rank + 1, total
            ));
        }
    }
}

pub fn absorb_non_claude_into_claude(claude_blocks: &[TimeBlock], non_claude_blocks: &mut Vec<TimeBlock>) {
    let mut result: Vec<TimeBlock> = Vec::new();

    for nc in non_claude_blocks.iter() {
        let mut segments = vec![(nc.start_ts, nc.end_ts)];

        for cb in claude_blocks {
            let mut next = Vec::new();
            for (start, end) in segments {
                if start >= cb.end_ts || end <= cb.start_ts {
                    next.push((start, end));
                } else {
                    if start < cb.start_ts {
                        next.push((start, cb.start_ts));
                    }
                    if end > cb.end_ts {
                        next.push((cb.end_ts, end));
                    }
                }
            }
            segments = next;
        }

        for (start, end) in segments {
            let mut clipped = nc.clone();
            clipped.start_ts = start;
            clipped.end_ts = end;
            clipped.duration_minutes = (end - start).num_seconds() as f64 / 60.0;
            result.push(clipped);
        }
    }

    *non_claude_blocks = result;
}

pub fn union_non_claude_blocks(blocks: &mut Vec<TimeBlock>) {
    if blocks.len() <= 1 { return; }
    blocks.sort_by_key(|b| b.start_ts);

    let mut merged: Vec<TimeBlock> = vec![blocks[0].clone()];
    for block in &blocks[1..] {
        let last = merged.last_mut().unwrap();
        if block.start_ts <= last.end_ts {
            if block.end_ts > last.end_ts {
                last.end_ts = block.end_ts;
                last.duration_minutes = (last.end_ts - last.start_ts).num_seconds() as f64 / 60.0;
                if let (Some(ids), Some(new_ids)) = (&mut last.source_event_ids, &block.source_event_ids) {
                    ids.push(',');
                    ids.push_str(new_ids);
                }
            }
        } else {
            merged.push(block.clone());
        }
    }
    *blocks = merged;
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use super::*;

    fn claude_block(start: &str, end: &str, session_id: &str) -> TimeBlock {
        TimeBlock {
            id: None, billing_project_id: 1, source: "claude".into(),
            start_ts: start.parse().unwrap(), end_ts: end.parse().unwrap(),
            duration_minutes: 0.0, billing_rate: BillingRate::Active,
            rate_multiplier: 1.0, cost_usd: Some(1.0),
            parallel_index: 0, parallel_total: 1, parallel_label: None,
            source_event_ids: Some(session_id.into()), computed_at: Utc::now(),
        }
    }

    fn nc_block(start: &str, end: &str, source: &str) -> TimeBlock {
        TimeBlock {
            id: None, billing_project_id: 1, source: source.into(),
            start_ts: start.parse().unwrap(), end_ts: end.parse().unwrap(),
            duration_minutes: 30.0, billing_rate: BillingRate::Active,
            rate_multiplier: 1.0, cost_usd: None,
            parallel_index: 0, parallel_total: 1, parallel_label: None,
            source_event_ids: None, computed_at: Utc::now(),
        }
    }

    #[test]
    fn detects_two_parallel_sessions() {
        let mut blocks = vec![
            claude_block("2026-05-06T10:00:00Z", "2026-05-06T10:30:00Z", "s1"),
            claude_block("2026-05-06T10:15:00Z", "2026-05-06T10:45:00Z", "s2"),
        ];
        detect_parallel_claude_sessions(&mut blocks);
        assert_eq!(blocks[0].parallel_total, 2);
        assert_eq!(blocks[1].parallel_total, 2);
        assert!(blocks[0].parallel_label.as_ref().unwrap().contains("1 of 2"));
    }

    #[test]
    fn non_overlapping_sessions_stay_solo() {
        let mut blocks = vec![
            claude_block("2026-05-06T10:00:00Z", "2026-05-06T10:30:00Z", "s1"),
            claude_block("2026-05-06T11:00:00Z", "2026-05-06T11:30:00Z", "s2"),
        ];
        detect_parallel_claude_sessions(&mut blocks);
        assert_eq!(blocks[0].parallel_total, 1);
        assert!(blocks[0].parallel_label.is_none());
    }

    #[test]
    fn detects_three_transitive_overlapping_sessions() {
        let mut blocks = vec![
            claude_block("2026-05-06T10:00:00Z", "2026-05-06T10:30:00Z", "s1"),
            claude_block("2026-05-06T10:20:00Z", "2026-05-06T10:50:00Z", "s2"),
            claude_block("2026-05-06T10:40:00Z", "2026-05-06T11:10:00Z", "s3"),
        ];
        detect_parallel_claude_sessions(&mut blocks);
        assert_eq!(blocks[0].parallel_total, 3);
        assert_eq!(blocks[1].parallel_total, 3);
        assert_eq!(blocks[2].parallel_total, 3);
        assert_eq!(blocks[0].parallel_index, 0);
        assert_eq!(blocks[1].parallel_index, 1);
        assert_eq!(blocks[2].parallel_index, 2);
    }

    #[test]
    fn independent_pair_unaffected_by_third() {
        let mut blocks = vec![
            claude_block("2026-05-06T10:00:00Z", "2026-05-06T10:30:00Z", "s1"),
            claude_block("2026-05-06T10:20:00Z", "2026-05-06T10:50:00Z", "s2"),
            claude_block("2026-05-06T12:00:00Z", "2026-05-06T12:30:00Z", "s3"),
        ];
        detect_parallel_claude_sessions(&mut blocks);
        assert_eq!(blocks[0].parallel_total, 2);
        assert_eq!(blocks[1].parallel_total, 2);
        assert_eq!(blocks[2].parallel_total, 1);
        assert!(blocks[2].parallel_label.is_none());
    }

    #[test]
    fn absorbs_non_claude_within_claude() {
        let claude = vec![claude_block("2026-05-06T10:00:00Z", "2026-05-06T11:00:00Z", "s1")];
        let mut non_claude = vec![nc_block("2026-05-06T10:15:00Z", "2026-05-06T10:45:00Z", "github")];
        absorb_non_claude_into_claude(&claude, &mut non_claude);
        assert!(non_claude.is_empty());
    }

    #[test]
    fn clips_partial_overlap_extending_past_claude() {
        let claude = vec![claude_block("2026-05-06T10:00:00Z", "2026-05-06T11:00:00Z", "s1")];
        let mut non_claude = vec![nc_block("2026-05-06T10:30:00Z", "2026-05-06T11:30:00Z", "github")];
        absorb_non_claude_into_claude(&claude, &mut non_claude);
        assert_eq!(non_claude.len(), 1);
        assert_eq!(non_claude[0].start_ts, "2026-05-06T11:00:00Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(non_claude[0].end_ts, "2026-05-06T11:30:00Z".parse::<DateTime<Utc>>().unwrap());
        assert!((non_claude[0].duration_minutes - 30.0).abs() < 0.01);
    }

    #[test]
    fn clips_partial_overlap_starting_before_claude() {
        let claude = vec![claude_block("2026-05-06T10:00:00Z", "2026-05-06T11:00:00Z", "s1")];
        let mut non_claude = vec![nc_block("2026-05-06T09:30:00Z", "2026-05-06T10:30:00Z", "github")];
        absorb_non_claude_into_claude(&claude, &mut non_claude);
        assert_eq!(non_claude.len(), 1);
        assert_eq!(non_claude[0].start_ts, "2026-05-06T09:30:00Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(non_claude[0].end_ts, "2026-05-06T10:00:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn keeps_non_claude_outside_claude() {
        let claude = vec![claude_block("2026-05-06T10:00:00Z", "2026-05-06T10:30:00Z", "s1")];
        let mut non_claude = vec![nc_block("2026-05-06T11:00:00Z", "2026-05-06T11:30:00Z", "github")];
        absorb_non_claude_into_claude(&claude, &mut non_claude);
        assert_eq!(non_claude.len(), 1);
    }

    #[test]
    fn unions_overlapping_non_claude() {
        let mut blocks = vec![
            nc_block("2026-05-06T14:00:00Z", "2026-05-06T14:30:00Z", "gdrive"),
            nc_block("2026-05-06T14:20:00Z", "2026-05-06T14:50:00Z", "slack"),
        ];
        union_non_claude_blocks(&mut blocks);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].end_ts, "2026-05-06T14:50:00Z".parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn non_overlapping_non_claude_stays_separate() {
        let mut blocks = vec![
            nc_block("2026-05-06T14:00:00Z", "2026-05-06T14:30:00Z", "gdrive"),
            nc_block("2026-05-06T16:00:00Z", "2026-05-06T16:30:00Z", "slack"),
        ];
        union_non_claude_blocks(&mut blocks);
        assert_eq!(blocks.len(), 2);
    }
}
