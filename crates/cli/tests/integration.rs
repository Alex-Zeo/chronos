use chronos_core::entities::*;
use chronos_core::store::ChronosStore;
use chronos_core::attribution::*;
use chronos_core::windows::*;
use chronos_core::overlap::*;
use chronos_report::summary::summarize;
use chronos_report::renderer::render_markdown;

#[test]
fn full_pipeline_ingest_to_report() {
    let store = ChronosStore::in_memory().unwrap();

    let client = Client {
        id: None, name: "Test Client".into(), contact: None,
        rate_usd_hr: 150.0, created_at: chrono::Utc::now(), notes: None,
    };
    let client_id = store.insert_client(&client).unwrap();

    let project = BillingProject {
        id: None, client_id, name: "Widget Build".into(),
        billing_type: BillingType::Hourly, rate_override: None,
        budget_hours: Some(40.0), status: ProjectStatus::Active,
        created_at: chrono::Utc::now(), goals_json: None,
    };
    let project_id = store.insert_project(&project).unwrap();

    let session1 = ClaudeSession {
        session_id: "s1".into(), project_id,
        start: "2026-05-06T09:00:00Z".parse().unwrap(),
        end: "2026-05-06T09:20:00Z".parse().unwrap(),
        session_type: "User".into(), cost_usd: 0.40,
    };
    let session2 = ClaudeSession {
        session_id: "s2".into(), project_id,
        start: "2026-05-06T10:00:00Z".parse().unwrap(),
        end: "2026-05-06T11:00:00Z".parse().unwrap(),
        session_type: "User".into(), cost_usd: 1.20,
    };
    let hook = ClaudeSession {
        session_id: "h1".into(), project_id,
        start: "2026-05-06T09:30:00Z".parse().unwrap(),
        end: "2026-05-06T09:31:00Z".parse().unwrap(),
        session_type: "Hook".into(), cost_usd: 0.01,
    };

    let mut all_blocks = vec![];
    all_blocks.extend(attribute_claude_session(&session1, 150.0));
    all_blocks.extend(attribute_claude_session(&session2, 150.0));
    all_blocks.extend(attribute_claude_session(&hook, 150.0));

    let commits = vec![
        PointEvent { timestamp: "2026-05-06T14:00:00Z".parse().unwrap(), source_event_id: "c1".into() },
        PointEvent { timestamp: "2026-05-06T14:10:00Z".parse().unwrap(), source_event_id: "c2".into() },
    ];
    let mut github_blocks = build_continuation_blocks(&commits, project_id, "github");

    let claude_blocks: Vec<TimeBlock> = all_blocks.iter()
        .filter(|b| b.source == "claude").cloned().collect();
    absorb_non_claude_into_claude(&claude_blocks, &mut github_blocks);
    union_non_claude_blocks(&mut github_blocks);

    all_blocks.extend(github_blocks);
    detect_parallel_claude_sessions(&mut all_blocks);

    for block in &all_blocks {
        store.insert_time_block(block).unwrap();
    }

    let summary = summarize(&all_blocks, 150.0);
    assert!(summary.total_active_minutes > 0.0);
    assert!(summary.total_cost_usd > 0.0);
    assert!(summary.by_source.contains_key("claude"));
    assert!(summary.by_source.contains_key("github"));

    let md = render_markdown(
        "Test Client", "Widget Build", "2026-W19",
        &summary, &[], &[], &[],
    );
    assert!(md.contains("Test Client"));
    assert!(md.contains("Widget Build"));
    assert!(md.contains("Billable hours"));
}
