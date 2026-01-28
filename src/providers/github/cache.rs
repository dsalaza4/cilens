use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use log::{debug, info};
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::types::GitHubJob;

/// Cached job data for GitHub Actions.
///
/// Jobs are stored in a `HashMap` with job ID as key for efficient lookups and deduplication.
/// Completed jobs are immutable and can be cached indefinitely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCache {
    /// All cached jobs for the repository, indexed by job ID
    pub jobs: HashMap<u64, GitHubJob>,
}

impl JobCache {
    /// Creates a new job cache.
    pub fn new(jobs: HashMap<u64, GitHubJob>) -> Self {
        Self { jobs }
    }

    /// Loads cache from disk for a specific repository.
    ///
    /// # Arguments
    ///
    /// * `repository` - GitHub repository (e.g., "owner/repo")
    ///
    /// # Returns
    ///
    /// Cached data if file exists and is valid, `None` otherwise
    pub fn load(repository: &str) -> Option<Self> {
        Self::load_with_base(repository, None)
    }

    /// Loads cache from disk with an optional base directory (used for testing).
    fn load_with_base(repository: &str, base_dir: Option<PathBuf>) -> Option<Self> {
        let cache_file = Self::get_cache_file_path_with_base(repository, base_dir).ok()?;

        if !cache_file.exists() {
            debug!("No cache file found for repository: {repository}");
            return None;
        }

        let content = fs::read_to_string(&cache_file).ok()?;
        let cache: Self = serde_json::from_str(&content).ok()?;

        debug!(
            "Loaded cache from: {} ({} jobs)",
            cache_file.display(),
            cache.jobs.len()
        );

        Some(cache)
    }

    /// Saves cache to disk for a specific repository.
    ///
    /// # Arguments
    ///
    /// * `repository` - GitHub repository (e.g., "owner/repo")
    ///
    /// # Errors
    ///
    /// Returns error if cache directory cannot be created or file cannot be written
    pub fn save(&self, repository: &str) -> Result<()> {
        self.save_with_base(repository, None)
    }

    /// Saves cache to disk with an optional base directory (used for testing).
    fn save_with_base(&self, repository: &str, base_dir: Option<PathBuf>) -> Result<()> {
        let cache_file = Self::get_cache_file_path_with_base(repository, base_dir)?;

        // Ensure parent directory exists
        if let Some(parent) = cache_file.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string(self)?;
        fs::write(&cache_file, content)?;

        info!(
            "Saved cache to: {} ({} jobs)",
            cache_file.display(),
            self.jobs.len()
        );

        Ok(())
    }

    /// Clears cached data for a specific repository.
    ///
    /// Removes the repository's cache file from disk.
    ///
    /// # Arguments
    ///
    /// * `repository` - GitHub repository (e.g., "owner/repo")
    ///
    /// # Errors
    ///
    /// Returns an error if cache file cannot be removed.
    pub fn clear(repository: &str) -> Result<()> {
        Self::clear_with_base(repository, None)
    }

    /// Clears cache with an optional base directory (used for testing).
    fn clear_with_base(repository: &str, base_dir: Option<PathBuf>) -> Result<()> {
        let cache_file = Self::get_cache_file_path_with_base(repository, base_dir)?;

        if cache_file.exists() {
            fs::remove_file(&cache_file)?;
            info!("Cache cleared: {}", cache_file.display());
        } else {
            info!("No cache file found for repository: {repository}");
        }

        Ok(())
    }

    /// Gets the cache file path with an optional base directory.
    ///
    /// Cache location: `<cache_dir>/cilens/github/<owner>-<repo>.json`
    /// (or platform equivalent)
    ///
    /// # Arguments
    ///
    /// * `repository` - GitHub repository (e.g., "owner/repo")
    /// * `base_dir` - Optional base cache directory (uses platform-specific if `None`)
    fn get_cache_file_path_with_base(
        repository: &str,
        base_dir: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let cache_base = if let Some(base) = base_dir {
            base
        } else {
            dirs::cache_dir().ok_or_else(|| {
                crate::error::CILensError::Cache("No cache directory found".into())
            })?
        };

        let cache_dir = cache_base
            .join("cilens")
            .join("github")
            .join(format!("{}.json", repository.replace('/', "-")));

        Ok(cache_dir)
    }
}
