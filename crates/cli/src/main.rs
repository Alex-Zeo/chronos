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
    },
    Report {
        #[command(subcommand)]
        action: ReportAction,
    },
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
        #[arg(long)]
        period: String,
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
        Commands::Ingest { since } => handle_ingest(&since, &store),
        Commands::Report { action } => {
            match action {
                ReportAction::Generate { project, period } => {
                    println!("Report for {project} period {period} (not yet implemented)");
                    Ok(())
                }
            }
        }
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

fn handle_ingest(since_str: &str, store: &ChronosStore) -> Result<()> {
    let since = parse_since(since_str)?;
    println!("Ingesting activity since {since}");

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
    for raw in &raw_events {
        let event = chronos_core::entities::ActivityEvent {
            id: None,
            source: raw.source.clone(),
            source_event_id: raw.source_event_id.clone(),
            billing_project_id: None,
            event_type: raw.event_type.clone(),
            timestamp: raw.timestamp,
            end_timestamp: raw.end_timestamp,
            actor: raw.actor.clone(),
            summary: raw.summary.clone(),
            metadata_json: raw.metadata_json.clone(),
            preliminary_project_id: None,
            needs_llm_review: false,
            ingested_at: now,
        };
        store.insert_activity_event(&event)?;
        ingested += 1;
    }

    store.upsert_cursor("claude", &now.to_rfc3339())?;
    println!("  Ingested {ingested} events total");
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
