use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use chronos_core::entities::{BillingType, Client, BillingProject, ProjectStatus};
use chronos_core::store::ChronosStore;

#[derive(Parser)]
#[command(name = "chronos", about = "Automated time tracking and billing")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    Ingest {
        #[arg(long, default_value = "7d")]
        since: String,
        #[arg(long)]
        project: Option<String>,
    },
    Report {
        #[command(subcommand)]
        action: ReportAction,
    },
    Demo {
        #[command(subcommand)]
        action: DemoAction,
    },
}

#[derive(Subcommand)]
enum DemoAction {
    Seed,
}

#[derive(Subcommand)]
enum ProjectAction {
    Create {
        name: String,
        #[arg(long)]
        client: String,
        #[arg(long, default_value = "hourly")]
        billing_type: String,
        #[arg(long)]
        rate: Option<f64>,
    },
    List,
    Link {
        #[arg(long)]
        project: String,
        #[arg(long)]
        github: Option<String>,
        #[arg(long)]
        drive: Option<String>,
        #[arg(long)]
        slack: Option<String>,
    },
}

#[derive(Subcommand)]
enum ReportAction {
    Generate {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = "7d")]
        since: String,
        #[arg(long)]
        output: Option<String>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "chronos=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();
    let db_path = ChronosStore::default_path()?;
    let store = ChronosStore::open(&db_path)?;

    match cli.command {
        Commands::Project { action } => handle_project(action, &store),
        Commands::Ingest { since, project } => handle_ingest(&since, project.as_deref(), &store),
        Commands::Report { action } => match action {
            ReportAction::Generate { project, since, output } => {
                handle_report(&project, &since, output.as_deref(), &store)
            }
        },
        Commands::Demo { action } => match action {
            DemoAction::Seed => handle_demo_seed(&store),
        },
    }
}

fn handle_project(action: ProjectAction, store: &ChronosStore) -> Result<()> {
    match action {
        ProjectAction::Create { name, client, billing_type, rate } => {
            let clients = store.list_clients()?;
            let client_id = match clients.iter().find(|c| c.name == client) {
                Some(c) => c.id.unwrap(),
                None => {
                    let c = Client {
                        id: None,
                        name: client.clone(),
                        contact: None,
                        rate_usd_hr: rate.unwrap_or(0.0),
                        created_at: chrono::Utc::now(),
                        notes: None,
                    };
                    store.insert_client(&c)?
                }
            };

            let bt = BillingType::from_str(&billing_type)
                .context("invalid billing type: use hourly, fixed, or compute_only")?;

            let project = BillingProject {
                id: None,
                client_id,
                name: name.clone(),
                billing_type: bt,
                rate_override: rate,
                budget_hours: None,
                status: ProjectStatus::Active,
                created_at: chrono::Utc::now(),
                goals_json: None,
            };
            let id = store.insert_project(&project)?;
            println!("Created project '{name}' (id={id}) for client '{client}'");
            Ok(())
        }
        ProjectAction::List => {
            let clients = store.list_clients()?;
            if clients.is_empty() {
                println!("No clients yet. Create one with: chronos project create --client <name> <project-name>");
                return Ok(());
            }
            for client in &clients {
                println!("\n{} (${}/hr)", client.name, client.rate_usd_hr);
                let projects = store.list_projects(client.id)?;
                if projects.is_empty() {
                    println!("  (no projects)");
                } else {
                    for p in &projects {
                        println!("  - {} [{}] ({})", p.name, p.billing_type.as_str(), p.status.as_str());
                    }
                }
            }
            Ok(())
        }
        ProjectAction::Link { project, github, drive, slack } => {
            println!("Linking project '{project}': github={github:?} drive={drive:?} slack={slack:?}");
            println!("(attribution rules not yet implemented)");
            Ok(())
        }
    }
}

fn handle_ingest(since_str: &str, project_name: Option<&str>, store: &ChronosStore) -> Result<()> {
    let since = parse_since(since_str)?;
    println!("Ingesting activity since {}", since.format("%Y-%m-%d %H:%M UTC"));

    let project = if let Some(name) = project_name {
        let projects = store.list_projects(None)?;
        let p = projects.into_iter().find(|p| p.name == name)
            .with_context(|| format!("no billing project named '{name}'. Create one first with: chronos project create --client <client> \"{name}\""))?;
        Some(p)
    } else {
        None
    };

    let bloomnet_path = rusty_data::connection::default_bloomnet_path()?;
    if !bloomnet_path.exists() {
        anyhow::bail!("bloomnet.db not found at {}", bloomnet_path.display());
    }

    let bloomnet = rusty_data::bloomnet::BloomnetStore::open_readonly(&bloomnet_path)?;
    let claude = chronos_connectors::claude::ClaudeConnector::new(bloomnet);

    use chronos_connectors::SourceConnector;
    let raw_events = claude.fetch_since(since)?;
    println!("  Claude: {} sessions fetched", raw_events.len());

    let now = chrono::Utc::now();
    let mut ingested = 0;
    let mut time_blocks = vec![];

    for raw in &raw_events {
        let project_id = project.as_ref().and_then(|p| p.id);

        let event = chronos_core::entities::ActivityEvent {
            id: None,
            source: raw.source.clone(),
            source_event_id: raw.source_event_id.clone(),
            billing_project_id: project_id,
            event_type: raw.event_type.clone(),
            timestamp: raw.timestamp,
            end_timestamp: raw.end_timestamp,
            actor: raw.actor.clone(),
            summary: raw.summary.clone(),
            metadata_json: raw.metadata_json.clone(),
            preliminary_project_id: project_id,
            needs_llm_review: false,
            ingested_at: now,
        };
        store.insert_activity_event(&event)?;
        ingested += 1;

        if let Some(ref proj) = project {
            let meta: serde_json::Value = serde_json::from_str(
                raw.metadata_json.as_deref().unwrap_or("{}")
            ).unwrap_or_default();
            let session_type = meta["session_type"].as_str().unwrap_or("User");
            let cost = meta["cost_usd"].as_f64().unwrap_or(0.0);

            let session = chronos_core::attribution::ClaudeSession {
                session_id: raw.source_event_id.clone(),
                project_id: proj.id.unwrap(),
                start: raw.timestamp,
                end: raw.end_timestamp.unwrap_or(raw.timestamp),
                session_type: session_type.into(),
                cost_usd: cost,
            };
            let rate = proj.rate_override.unwrap_or(0.0);
            let blocks = chronos_core::attribution::attribute_claude_session(&session, rate);
            time_blocks.extend(blocks);
        }
    }

    chronos_core::overlap::detect_parallel_claude_sessions(&mut time_blocks);

    let block_count = time_blocks.len();
    for block in &time_blocks {
        store.insert_time_block(block)?;
    }

    store.upsert_cursor("claude", &now.to_rfc3339())?;
    println!("  Ingested {ingested} events, generated {block_count} time blocks");
    Ok(())
}

fn handle_report(project_name: &str, since_str: &str, output: Option<&str>, store: &ChronosStore) -> Result<()> {
    let since = parse_since(since_str)?;
    let now = chrono::Utc::now();

    let projects = store.list_projects(None)?;
    let project = projects.iter().find(|p| p.name == project_name)
        .with_context(|| format!("no billing project named '{project_name}'"))?;
    let project_id = project.id.unwrap();

    let clients = store.list_clients()?;
    let client = clients.iter().find(|c| c.id == Some(project.client_id))
        .context("client not found")?;

    let blocks = store.list_time_blocks_for_project(project_id, since, now)?;
    let events = store.list_activity_events_for_project(project_id, since, now)?;

    if blocks.is_empty() {
        println!("No time blocks found for '{project_name}' since {}.", since.format("%Y-%m-%d"));
        println!("Run: chronos ingest --project \"{project_name}\" --since {since_str}");
        return Ok(());
    }

    let rate = project.rate_override.unwrap_or(client.rate_usd_hr);
    let summary = chronos_report::summary::summarize(&blocks, rate);
    let parallel_blocks: Vec<_> = blocks.iter().filter(|b| b.parallel_index > 0).cloned().collect();
    let period = format!("{} to {}", since.format("%Y-%m-%d"), now.format("%Y-%m-%d"));

    println!("  {} time blocks, {:.1} billable hours, ${:.2} total",
        blocks.len(), summary.billable_hours, summary.total_cost_usd);

    match output {
        Some(path) if path.ends_with(".pdf") => {
            let html = chronos_report::renderer::render_html(
                &client.name, &project.name, &period, &summary, &events, &[], &parallel_blocks,
            );
            let abs_pdf = std::fs::canonicalize(".").unwrap().join(path);
            let html_tmp = abs_pdf.with_extension("tmp.html");
            std::fs::write(&html_tmp, &html)?;

            let file_url = format!("file://{}", html_tmp.display());
            let status = std::process::Command::new(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
            )
                .args([
                    "--headless",
                    "--disable-gpu",
                    "--no-pdf-header-footer",
                    &format!("--print-to-pdf={}", abs_pdf.display()),
                    &file_url,
                ])
                .status()?;

            std::fs::remove_file(&html_tmp).ok();

            if status.success() {
                println!("  PDF saved to {path}");
            } else {
                anyhow::bail!("Chrome PDF generation failed (exit {})", status.code().unwrap_or(-1));
            }
        }
        Some(path) if path.ends_with(".html") => {
            let html = chronos_report::renderer::render_html(
                &client.name, &project.name, &period, &summary, &events, &[], &parallel_blocks,
            );
            std::fs::write(path, &html)?;
            println!("  HTML saved to {path}");
        }
        Some(path) => {
            let md = chronos_report::renderer::render_markdown(
                &client.name, &project.name, &period, &summary, &events, &[], &parallel_blocks,
            );
            std::fs::write(path, &md)?;
            println!("  Report saved to {path}");
        }
        None => {
            let md = chronos_report::renderer::render_markdown(
                &client.name, &project.name, &period, &summary, &events, &[], &parallel_blocks,
            );
            println!("{md}");
        }
    }

    Ok(())
}

fn handle_demo_seed(store: &ChronosStore) -> Result<()> {
    use chronos_core::entities::*;

    let now = chrono::Utc::now();

    let client_id = store.insert_client(&Client {
        id: None, name: "Acme Corp".into(),
        contact: Some("alice@acme.com".into()),
        rate_usd_hr: 150.0, created_at: now,
        notes: Some("Demo client".into()),
    })?;

    let project_id = store.insert_project(&BillingProject {
        id: None, client_id, name: "Website Redesign".into(),
        billing_type: BillingType::Hourly, rate_override: None,
        budget_hours: Some(80.0), status: ProjectStatus::Active,
        created_at: now,
        goals_json: Some(r#"["Redesign homepage","Migrate to Next.js","Launch by Q3"]"#.into()),
    })?;

    let base = now - chrono::Duration::days(7);
    let sessions = [
        ("demo-s1", "User", 0i64, 25i64, 0.45f64),
        ("demo-s2", "User", 2, 60, 1.20),
        ("demo-s3", "Hook", 3, 1, 0.01),
        ("demo-s4", "User", 4, 45, 0.95),
        ("demo-s5", "Subagent", 4, 10, 0.15),
    ];

    for (id, stype, day_offset, duration_min, cost) in sessions {
        let start = base + chrono::Duration::days(day_offset) + chrono::Duration::hours(9);
        let end = start + chrono::Duration::minutes(duration_min);

        let session = chronos_core::attribution::ClaudeSession {
            session_id: id.into(), project_id,
            start, end, session_type: stype.into(), cost_usd: cost,
        };
        let blocks = chronos_core::attribution::attribute_claude_session(&session, 150.0);
        for block in &blocks {
            store.insert_time_block(block)?;
        }

        store.insert_activity_event(&ActivityEvent {
            id: None, source: "claude".into(), source_event_id: id.into(),
            billing_project_id: Some(project_id),
            event_type: format!("claude_session_{}", stype.to_lowercase()),
            timestamp: start, end_timestamp: Some(end),
            actor: Some("demo-user".into()),
            summary: Some(format!("{stype} session on Website Redesign")),
            metadata_json: Some(serde_json::json!({"cost_usd": cost}).to_string()),
            preliminary_project_id: Some(project_id),
            needs_llm_review: false, ingested_at: now,
        })?;
    }

    let commit_events = vec![
        chronos_core::windows::PointEvent {
            timestamp: base + chrono::Duration::days(1) + chrono::Duration::hours(14),
            source_event_id: "demo-c1".into(),
        },
        chronos_core::windows::PointEvent {
            timestamp: base + chrono::Duration::days(1) + chrono::Duration::hours(14) + chrono::Duration::minutes(5),
            source_event_id: "demo-c2".into(),
        },
    ];
    let github_blocks = chronos_core::windows::build_continuation_blocks(&commit_events, project_id, "github");
    for block in &github_blocks {
        store.insert_time_block(block)?;
    }

    println!("Demo data seeded:");
    println!("  Client: Acme Corp ($150/hr)");
    println!("  Project: Website Redesign (80hr budget)");
    println!("  {} Claude sessions, {} GitHub commit clusters", sessions.len(), 1);
    println!("\nTry: chronos project list");

    Ok(())
}

fn parse_since(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = s.parse::<chrono::DateTime<chrono::Utc>>() {
        return Ok(dt);
    }
    let s = s.trim();
    if let Some(num_str) = s.strip_suffix('d') {
        let days: i64 = num_str.parse().context("invalid day count")?;
        return Ok(chrono::Utc::now() - chrono::Duration::days(days));
    }
    if let Some(num_str) = s.strip_suffix('h') {
        let hours: i64 = num_str.parse().context("invalid hour count")?;
        return Ok(chrono::Utc::now() - chrono::Duration::hours(hours));
    }
    anyhow::bail!("cannot parse '{s}' as duration (use 7d, 24h) or ISO 8601 timestamp")
}
