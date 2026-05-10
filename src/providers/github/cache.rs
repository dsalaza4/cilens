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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[fixtura::test]
    fn test_cache_new(#[fixtura(id = 1u64, name = "build".to_string())] job: GitHubJob) {
        let mut jobs = HashMap::new();
        jobs.insert(job.id, job);
        let cache = JobCache::new(jobs);
        assert_eq!(cache.jobs.len(), 1);
        assert_eq!(cache.jobs.get(&1).unwrap().name, "build");
    }

    #[fixtura::test]
    fn test_cache_save_and_load_roundtrip(
        #[fixtura(id = 1u64, name = "build".to_string())] job1: GitHubJob,
        #[fixtura(id = 2u64, name = "test".to_string())] job2: GitHubJob,
        #[fixtura(id = 3u64, name = "deploy".to_string())] job3: GitHubJob,
    ) {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let mut map = HashMap::new();
        map.insert(job1.id, job1);
        map.insert(job2.id, job2);
        map.insert(job3.id, job3);

        let cache = JobCache::new(map);
        let repository = "owner/repo";

        cache
            .save_with_base(repository, Some(base_dir.clone()))
            .unwrap();

        let loaded = JobCache::load_with_base(repository, Some(base_dir)).unwrap();
        assert_eq!(loaded.jobs.len(), 3);
        assert_eq!(loaded.jobs.get(&1).unwrap().name, "build");
        assert_eq!(loaded.jobs.get(&2).unwrap().name, "test");
        assert_eq!(loaded.jobs.get(&3).unwrap().name, "deploy");
    }

    #[test]
    fn test_cache_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let loaded = JobCache::load_with_base("owner/nonexistent", Some(base_dir));
        assert!(loaded.is_none());
    }

    #[fixtura::test]
    fn test_cache_clear_existing(job: GitHubJob) {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let mut jobs = HashMap::new();
        jobs.insert(job.id, job);

        let cache = JobCache::new(jobs);
        let repository = "owner/repo";

        cache
            .save_with_base(repository, Some(base_dir.clone()))
            .unwrap();

        assert!(JobCache::load_with_base(repository, Some(base_dir.clone())).is_some());

        JobCache::clear_with_base(repository, Some(base_dir.clone())).unwrap();

        assert!(JobCache::load_with_base(repository, Some(base_dir)).is_none());
    }

    #[test]
    fn test_cache_file_path_format() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let path =
            JobCache::get_cache_file_path_with_base("owner/repo", Some(base_dir.clone())).unwrap();

        assert!(path.ends_with("cilens/github/owner-repo.json"));
        assert!(path.starts_with(&base_dir));
    }

    #[test]
    fn test_cache_empty_jobs() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let cache = JobCache::new(HashMap::new());
        let repository = "owner/repo";

        cache
            .save_with_base(repository, Some(base_dir.clone()))
            .unwrap();
        let loaded = JobCache::load_with_base(repository, Some(base_dir)).unwrap();

        assert_eq!(loaded.jobs.len(), 0);
    }
}
