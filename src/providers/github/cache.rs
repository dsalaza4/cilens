use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

use super::types::GitHubJob;
use crate::error::Result;

/// Internal structure for cached workflow run data
#[derive(Debug, Serialize, Deserialize)]
struct CachedWorkflowRun {
    jobs: Vec<GitHubJob>,
}

/// Cache for GitHub Actions workflow job data
#[derive(Debug)]
pub struct JobCache {
    cache_file: PathBuf,
    workflow_runs: HashMap<u64, CachedWorkflowRun>,
    enabled: bool,
}

impl JobCache {
    /// Create a new job cache for a GitHub repository
    ///
    /// # Arguments
    /// * `repo_path` - Repository path in format "owner/repo"
    /// * `enabled` - Whether caching is enabled
    ///
    /// # Returns
    /// * `Result<Self>` - The job cache instance
    pub fn new(repo_path: &str, enabled: bool) -> Result<Self> {
        if !enabled {
            debug!("Cache disabled for repository {repo_path}");
            return Ok(Self {
                cache_file: PathBuf::new(),
                workflow_runs: HashMap::new(),
                enabled: false,
            });
        }

        // Get cache directory
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| {
                crate::error::CILensError::Cache("Could not determine cache directory".to_string())
            })?
            .join("cilens")
            .join("github");

        // Create cache directory if it doesn't exist
        fs::create_dir_all(&cache_dir).map_err(|e| {
            crate::error::CILensError::Cache(format!("Failed to create cache directory: {e}"))
        })?;

        // Create cache file name: replace '/' with '-'
        let cache_file_name = format!("{}.json", repo_path.replace('/', "-"));
        let cache_file = cache_dir.join(cache_file_name);

        // Load existing cache if file exists
        let workflow_runs = if cache_file.exists() {
            match fs::read_to_string(&cache_file) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(data) => {
                        let display = cache_file.display();
                        info!("Loaded cache from {display}");
                        data
                    }
                    Err(e) => {
                        let display = cache_file.display();
                        warn!(
                            "Failed to parse cache file {display}: {e}. Starting with empty cache."
                        );
                        HashMap::new()
                    }
                },
                Err(e) => {
                    let display = cache_file.display();
                    warn!("Failed to read cache file {display}: {e}. Starting with empty cache.");
                    HashMap::new()
                }
            }
        } else {
            let display = cache_file.display();
            debug!("No cache file found at {display}");
            HashMap::new()
        };

        Ok(Self {
            cache_file,
            workflow_runs,
            enabled: true,
        })
    }

    /// Get cached jobs for a workflow run
    ///
    /// # Arguments
    /// * `run_id` - The workflow run ID
    ///
    /// # Returns
    /// * `Option<Vec<GitHubJob>>` - The cached jobs if available
    pub fn get(&self, run_id: u64) -> Option<Vec<GitHubJob>> {
        if !self.enabled {
            return None;
        }

        self.workflow_runs
            .get(&run_id)
            .map(|cached| cached.jobs.clone())
    }

    /// Insert jobs for a workflow run into the cache
    ///
    /// # Arguments
    /// * `run_id` - The workflow run ID
    /// * `jobs` - The jobs to cache
    pub fn insert(&mut self, run_id: u64, jobs: Vec<GitHubJob>) {
        if !self.enabled {
            return;
        }

        self.workflow_runs
            .insert(run_id, CachedWorkflowRun { jobs });
    }

    /// Save the cache to disk
    ///
    /// Should be called after all workflow runs have been fetched to persist the cache.
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    pub fn save(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let json = serde_json::to_string(&self.workflow_runs).map_err(|e| {
            crate::error::CILensError::Cache(format!("Failed to serialize cache: {e}"))
        })?;

        fs::write(&self.cache_file, json).map_err(|e| {
            crate::error::CILensError::Cache(format!("Failed to write cache file: {e}"))
        })?;

        let display = self.cache_file.display();
        info!("Saved cache to {display} ({} workflow runs)", self.workflow_runs.len());

        Ok(())
    }

    /// Clear the cache for a specific repository
    ///
    /// # Arguments
    /// * `repo_path` - Repository path in format "owner/repo"
    ///
    /// # Returns
    /// * `Result<()>` - Success or error
    pub fn clear_project_cache(repo_path: &str) -> Result<()> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| {
                crate::error::CILensError::Cache("Could not determine cache directory".to_string())
            })?
            .join("cilens")
            .join("github");

        let cache_file_name = format!("{}.json", repo_path.replace('/', "-"));
        let cache_file = cache_dir.join(cache_file_name);

        if cache_file.exists() {
            fs::remove_file(&cache_file).map_err(|e| {
                crate::error::CILensError::Cache(format!("Failed to delete cache file: {e}"))
            })?;
            info!("Cleared cache for repository {repo_path}");
        } else {
            debug!("No cache file to clear for repository {repo_path}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_creation() {
        // Test cache creation with enabled=true
        let result = JobCache::new("owner/repo", true);
        assert!(result.is_ok());

        let cache = result.unwrap();
        assert!(cache.enabled);
        assert!(cache
            .cache_file
            .to_string_lossy()
            .contains("owner-repo.json"));
        assert!(cache.workflow_runs.is_empty());
    }

    #[test]
    fn test_cache_disabled() {
        // Test cache with enabled=false
        let result = JobCache::new("owner/repo", false);
        assert!(result.is_ok());

        let cache = result.unwrap();
        assert!(!cache.enabled);
        assert!(cache.cache_file.as_os_str().is_empty());
        assert!(cache.workflow_runs.is_empty());

        // get() should return None when cache is disabled
        assert!(cache.get(12345).is_none());
    }

    #[test]
    fn test_cache_get_empty() {
        // Test getting from empty cache
        let cache = JobCache::new("owner/repo", true).unwrap();
        assert!(cache.get(12345).is_none());
    }

    #[test]
    fn test_cache_insert_and_get() {
        // Test inserting and retrieving from cache
        let mut cache = JobCache::new("test-owner/test-repo-insert", true).unwrap();

        let jobs = vec![
            GitHubJob {
                id: 1,
                name: "build".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some("2025-01-01T00:00:00Z".to_string()),
                completed_at: Some("2025-01-01T00:05:00Z".to_string()),
                needs: None,
            },
        ];

        cache.insert(12345, jobs.clone());

        let cached = cache.get(12345);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);

        // Clean up
        let _ = JobCache::clear_project_cache("test-owner/test-repo-insert");
    }

    #[test]
    fn test_cache_save_and_reload() {
        // Test saving cache to disk and reloading
        let repo = "test-owner/test-repo-save";
        let _ = JobCache::clear_project_cache(repo);

        let jobs = vec![
            GitHubJob {
                id: 1,
                name: "test".to_string(),
                status: "completed".to_string(),
                conclusion: Some("success".to_string()),
                started_at: Some("2025-01-01T00:00:00Z".to_string()),
                completed_at: Some("2025-01-01T00:03:00Z".to_string()),
                needs: None,
            },
        ];

        // Insert and save
        {
            let mut cache = JobCache::new(repo, true).unwrap();
            cache.insert(99999, jobs.clone());
            cache.save().unwrap();
        }

        // Reload and verify
        {
            let cache = JobCache::new(repo, true).unwrap();
            let cached = cache.get(99999);
            assert!(cached.is_some());
            assert_eq!(cached.unwrap().len(), 1);
        }

        // Clean up
        let _ = JobCache::clear_project_cache(repo);
    }

    #[test]
    fn test_clear_project_cache() {
        // Create a temporary cache file
        let cache_dir = dirs::cache_dir().unwrap().join("cilens").join("github");
        fs::create_dir_all(&cache_dir).unwrap();

        let test_repo = "test-owner/test-repo";
        let cache_file = cache_dir.join("test-owner-test-repo.json");
        fs::write(&cache_file, "{}").unwrap();

        // Verify file exists
        assert!(cache_file.exists());

        // Clear cache
        let result = JobCache::clear_project_cache(test_repo);
        assert!(result.is_ok());

        // Verify file is deleted
        assert!(!cache_file.exists());
    }
}
