//! GitHub API client — releases, workflow runs, artifacts.
//!
//! Blocking ureq, no auth. Process-wide 5-minute response cache keyed on URL,
//! storing raw JSON bodies so each caller reparses into its own type.

use std::sync::Arc;
use std::time::Duration;

use moka::sync::Cache;
use serde::Deserialize;

use crate::error::{LtboxError, Result};

const API_BASE: &str = "https://api.github.com";

static RESPONSE_CACHE: std::sync::LazyLock<Cache<String, Arc<String>>> =
    std::sync::LazyLock::new(|| {
        Cache::builder()
            .time_to_live(Duration::from_secs(5 * 60))
            .max_capacity(128)
            .build()
    });

pub struct GitHubClient {
    owner_repo: String,
    agent: ureq::Agent,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct TagItem {
    name: String,
}

#[derive(Debug, Deserialize)]
struct WorkflowRunsResponse {
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub head_branch: Option<String>,
    pub path: Option<String>,
    pub status: Option<String>,
    pub conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtifactsResponse {
    artifacts: Vec<ArtifactInfo>,
}

#[derive(Debug, Deserialize)]
struct ArtifactInfo {
    name: String,
}

impl GitHubClient {
    pub fn new(owner_repo: &str) -> Result<Self> {
        let agent = crate::downloader::build_agent();
        Ok(Self {
            owner_repo: owner_repo.to_string(),
            agent,
        })
    }

    /// Parse "github.com/owner/repo" or "owner/repo" into "owner/repo".
    pub fn from_url(url: &str) -> Result<Self> {
        let repo = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("github.com/")
            .trim_end_matches('/')
            .to_string();
        if repo.matches('/').count() != 1 {
            return Err(LtboxError::Config(format!("Invalid repo: {url}")));
        }
        Self::new(&repo)
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        // Exponential backoff on transport / 5xx (100ms → 400ms → 1600ms).
        // 4xx short-circuits.
        let url = format!("{API_BASE}/repos/{}{endpoint}", self.owner_repo);

        if let Some(cached) = RESPONSE_CACHE.get(&url) {
            return serde_json::from_str::<T>(&cached)
                .map_err(|e| LtboxError::Download(format!("JSON parse error (cached): {e}")));
        }

        let mut last_err: Option<LtboxError> = None;
        for attempt in 0..3_u32 {
            if attempt > 0 {
                let delay_ms = 100u64 * 4u64.pow(attempt - 1);
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            match self.agent.get(&url).call() {
                Ok(mut resp) => {
                    // 1 MiB cap — GitHub release + tag JSON payloads we hit
                    // are well under this; any larger body on these endpoints
                    // is a red flag.
                    let body = resp
                        .body_mut()
                        .read_to_string()
                        .map_err(|e| LtboxError::Download(format!("read body: {e}")))?;
                    let parsed = serde_json::from_str::<T>(&body)
                        .map_err(|e| LtboxError::Download(format!("JSON parse error: {e}")))?;
                    // Only cache successful parses.
                    RESPONSE_CACHE.insert(url.clone(), Arc::new(body));
                    return Ok(parsed);
                }
                Err(ureq::Error::StatusCode(code)) => {
                    if (400..500).contains(&code) {
                        return Err(LtboxError::Download(format!("GitHub API {code}: {url}")));
                    }
                    last_err = Some(LtboxError::Download(format!("GitHub API {code}: {url}")));
                }
                Err(e) => {
                    last_err = Some(LtboxError::Download(format!("Request failed: {e}")));
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            LtboxError::Download("GitHub API exhausted retries with no recorded error".into())
        }))
    }

    pub fn latest_release_tag(&self) -> Result<String> {
        let release: Release = self.get_json("/releases/latest")?;
        Ok(release.tag_name)
    }

    /// Latest release: `(tag, [(asset_name, browser_download_url)])`.
    pub fn latest_release_assets(&self) -> Result<(String, Vec<(String, String)>)> {
        let release: Release = self.get_json("/releases/latest")?;
        let tag = release.tag_name;
        let assets = release
            .assets
            .into_iter()
            .map(|a| (a.name, a.browser_download_url))
            .collect();
        Ok((tag, assets))
    }

    /// First latest-release asset whose name matches `predicate` → `(name, url)`.
    pub fn latest_release_asset_where(
        &self,
        predicate: impl Fn(&str) -> bool,
    ) -> Result<(String, String)> {
        let (_tag, assets) = self.latest_release_assets()?;
        assets
            .into_iter()
            .find(|(name, _)| predicate(name))
            .ok_or_else(|| {
                LtboxError::Download(format!(
                    "No matching asset on latest release of {}",
                    self.owner_repo
                ))
            })
    }

    pub fn release_by_tag(&self, tag: &str) -> Result<Vec<(String, String)>> {
        let release: Release = self.get_json(&format!("/releases/tags/{tag}"))?;
        Ok(release
            .assets
            .into_iter()
            .map(|a| (a.name, a.browser_download_url))
            .collect())
    }

    /// Latest tag name (tags API, not releases).
    pub fn latest_tag(&self) -> Result<String> {
        let tags: Vec<TagItem> = self.get_json("/tags?per_page=1")?;
        tags.first()
            .map(|t| t.name.clone())
            .ok_or_else(|| LtboxError::Download("No tags found".into()))
    }

    pub fn workflow_run_for_tag(&self, tag: &str) -> Result<u64> {
        let resp: WorkflowRunsResponse = self.get_json(&format!(
            "/actions/runs?per_page=30&status=completed&branch={tag}"
        ))?;
        resp.workflow_runs
            .first()
            .map(|r| r.id)
            .ok_or_else(|| LtboxError::Download(format!("No workflow run for tag {tag}")))
    }

    pub fn workflow_artifacts(&self, run_id: u64) -> Result<Vec<String>> {
        let resp: ArtifactsResponse =
            self.get_json(&format!("/actions/runs/{run_id}/artifacts"))?;
        Ok(resp.artifacts.into_iter().map(|a| a.name).collect())
    }

    pub fn workflow_run_matches(
        &self,
        run_id: u64,
        workflow_file: &str,
        branch: Option<&str>,
    ) -> Result<bool> {
        let run: WorkflowRun = self.get_json(&format!("/actions/runs/{run_id}"))?;
        if let Some(b) = branch
            && run.head_branch.as_deref() != Some(b)
        {
            return Ok(false);
        }
        if !workflow_file.is_empty() {
            let expected = normalize_workflow_path(workflow_file);
            let actual = run
                .path
                .as_deref()
                .map(normalize_workflow_path)
                .unwrap_or_default();
            if actual != expected {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn latest_successful_run(
        &self,
        workflow_file: &str,
        branch: Option<&str>,
    ) -> Result<Option<u64>> {
        let mut endpoint =
            format!("/actions/workflows/{workflow_file}/runs?status=success&per_page=20");
        if let Some(b) = branch {
            endpoint.push_str(&format!("&branch={b}"));
        }
        let resp: WorkflowRunsResponse = self.get_json(&endpoint)?;
        Ok(resp.workflow_runs.first().map(|r| r.id))
    }
}

fn normalize_workflow_path(path: &str) -> String {
    path.trim_start_matches(".github/workflows/")
        .trim_start_matches(".github/workflows\\")
        .to_lowercase()
}
