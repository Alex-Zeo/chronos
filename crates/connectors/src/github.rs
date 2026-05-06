use chrono::{DateTime, Utc};
use crate::{ConnectorStatus, RawEvent, SourceConnector};

pub struct GitHubConnector {
    token: String,
    repos: Vec<String>,
    client: reqwest::Client,
}

impl GitHubConnector {
    pub fn new(token: String, repos: Vec<String>) -> Self {
        Self { token, repos, client: reqwest::Client::new() }
    }
}

impl SourceConnector for GitHubConnector {
    fn source_id(&self) -> &str { "github" }

    fn fetch_since(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| tokio::runtime::Runtime::new().unwrap().handle().clone());
        rt.block_on(async { self.fetch_async(since).await })
    }

    fn health_check(&self) -> ConnectorStatus {
        ConnectorStatus {
            source: "github".into(),
            healthy: !self.token.is_empty(),
            message: if self.token.is_empty() { "GITHUB_TOKEN not set".into() }
            else { format!("{} repos configured", self.repos.len()) },
            last_fetch: None,
        }
    }
}

impl GitHubConnector {
    async fn fetch_async(&self, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let mut all_events = vec![];
        for repo in &self.repos {
            all_events.extend(self.fetch_commits(repo, since).await?);
            all_events.extend(self.fetch_prs(repo, since).await?);
        }
        Ok(all_events)
    }

    async fn fetch_commits(&self, repo: &str, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let url = format!("https://api.github.com/repos/{repo}/commits?since={}", since.to_rfc3339());
        let resp = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "chronos")
            .header("Accept", "application/vnd.github+json")
            .send().await?;

        if !resp.status().is_success() {
            tracing::warn!("GitHub commits API returned {}", resp.status());
            return Ok(vec![]);
        }

        let commits: Vec<serde_json::Value> = resp.json().await?;
        let events = commits.into_iter().filter_map(|c| {
            let sha = c["sha"].as_str()?.to_string();
            let message = c["commit"]["message"].as_str().unwrap_or("").to_string();
            let date_str = c["commit"]["author"]["date"].as_str()?;
            let ts = date_str.parse::<DateTime<Utc>>().ok()?;
            let author = c["commit"]["author"]["name"].as_str().map(String::from);

            Some(RawEvent {
                source: "github".into(),
                source_event_id: format!("commit:{sha}"),
                event_type: "github_commit".into(),
                timestamp: ts,
                end_timestamp: None,
                actor: author,
                summary: Some(message),
                metadata_json: Some(serde_json::json!({"repo": repo, "sha": sha}).to_string()),
            })
        }).collect();
        Ok(events)
    }

    async fn fetch_prs(&self, repo: &str, since: DateTime<Utc>) -> anyhow::Result<Vec<RawEvent>> {
        let url = format!("https://api.github.com/repos/{repo}/pulls?state=all&sort=updated&direction=desc&per_page=50");
        let resp = self.client.get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "chronos")
            .header("Accept", "application/vnd.github+json")
            .send().await?;

        if !resp.status().is_success() { return Ok(vec![]); }

        let prs: Vec<serde_json::Value> = resp.json().await?;
        let events = prs.into_iter().filter_map(|pr| {
            let number = pr["number"].as_i64()?;
            let title = pr["title"].as_str()?.to_string();
            let created = pr["created_at"].as_str()?.parse::<DateTime<Utc>>().ok()?;
            if created < since { return None; }
            let merged = pr["merged_at"].as_str().and_then(|s| s.parse::<DateTime<Utc>>().ok());
            let author = pr["user"]["login"].as_str().map(String::from);

            Some(RawEvent {
                source: "github".into(),
                source_event_id: format!("pr:{repo}#{number}"),
                event_type: "github_pr".into(),
                timestamp: created,
                end_timestamp: merged,
                actor: author,
                summary: Some(title),
                metadata_json: Some(serde_json::json!({"repo": repo, "number": number}).to_string()),
            })
        }).collect();
        Ok(events)
    }
}
