use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use log::{debug, info};
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::types::GitLabJob;

/// Cached job data.
///
/// Jobs are stored in a `HashMap` with job ID as key for efficient lookups and deduplication.
/// Jobs with SUCCESS or FAILED status are immutable and can be cached indefinitely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCache {
    /// All cached jobs for the project, indexed by job ID
    pub jobs: HashMap<String, GitLabJob>,
}

impl JobCache {
    /// Creates a new job cache.
    pub fn new(jobs: HashMap<String, GitLabJob>) -> Self {
        Self { jobs }
    }

    /// Loads cache from disk for a specific project.
    ///
    /// # Arguments
    ///
    /// * `project_path` - GitLab project path (e.g., "group/project")
    ///
    /// # Returns
    ///
    /// Cached data if file exists and is valid, `None` otherwise
    pub fn load(project_path: &str) -> Option<Self> {
        Self::load_with_base(project_path, None)
    }

    /// Loads cache from disk with an optional base directory (used for testing).
    fn load_with_base(project_path: &str, base_dir: Option<PathBuf>) -> Option<Self> {
        let cache_file = Self::get_cache_file_path_with_base(project_path, base_dir).ok()?;

        if !cache_file.exists() {
            debug!("No cache file found for project: {project_path}");
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

    /// Saves cache to disk for a specific project.
    ///
    /// # Arguments
    ///
    /// * `project_path` - GitLab project path (e.g., "group/project")
    ///
    /// # Errors
    ///
    /// Returns error if cache directory cannot be created or file cannot be written
    pub fn save(&self, project_path: &str) -> Result<()> {
        self.save_with_base(project_path, None)
    }

    /// Saves cache to disk with an optional base directory (used for testing).
    fn save_with_base(&self, project_path: &str, base_dir: Option<PathBuf>) -> Result<()> {
        let cache_file = Self::get_cache_file_path_with_base(project_path, base_dir)?;

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

    /// Clears cached data for a specific project.
    ///
    /// Removes the project's cache file from disk.
    ///
    /// # Arguments
    ///
    /// * `project_path` - GitLab project path (e.g., "group/project")
    ///
    /// # Errors
    ///
    /// Returns an error if cache file cannot be removed.
    pub fn clear(project_path: &str) -> Result<()> {
        Self::clear_with_base(project_path, None)
    }

    /// Clears cache with an optional base directory (used for testing).
    fn clear_with_base(project_path: &str, base_dir: Option<PathBuf>) -> Result<()> {
        let cache_file = Self::get_cache_file_path_with_base(project_path, base_dir)?;

        if cache_file.exists() {
            fs::remove_file(&cache_file)?;
            info!("Cache cleared: {}", cache_file.display());
        } else {
            info!("No cache file found for project: {project_path}");
        }

        Ok(())
    }

    /// Gets the cache file path with an optional base directory.
    ///
    /// Cache location: `<cache_dir>/cilens/gitlab/<group>-<project>.json`
    /// (or platform equivalent)
    ///
    /// # Arguments
    ///
    /// * `project_path` - GitLab project path (e.g., "group/project")
    /// * `base_dir` - Optional base cache directory (uses platform-specific if `None`)
    fn get_cache_file_path_with_base(
        project_path: &str,
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
            .join("gitlab")
            .join(format!("{}.json", project_path.replace('/', "-")));

        Ok(cache_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[fixtura::test]
    fn test_cache_save_and_load_roundtrip(
        #[fixtura(id = "1".to_string(), name = "build".to_string())] job1: GitLabJob,
        #[fixtura(id = "2".to_string(), name = "test".to_string())] job2: GitLabJob,
        #[fixtura(id = "3".to_string(), name = "deploy".to_string())] job3: GitLabJob,
    ) {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let mut map = HashMap::new();
        map.insert(job1.id.clone(), job1);
        map.insert(job2.id.clone(), job2);
        map.insert(job3.id.clone(), job3);

        let cache = JobCache::new(map);
        let project_path = "group/project";

        cache
            .save_with_base(project_path, Some(base_dir.clone()))
            .unwrap();

        let loaded = JobCache::load_with_base(project_path, Some(base_dir)).unwrap();
        assert_eq!(loaded.jobs.len(), 3);
        assert_eq!(loaded.jobs.get("1").unwrap().name, "build");
        assert_eq!(loaded.jobs.get("2").unwrap().name, "test");
        assert_eq!(loaded.jobs.get("3").unwrap().name, "deploy");
    }

    #[test]
    fn test_cache_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let loaded = JobCache::load_with_base("nonexistent/project", Some(base_dir));
        assert!(loaded.is_none());
    }

    #[fixtura::test]
    fn test_cache_clear(job: GitLabJob) {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let mut map = HashMap::new();
        map.insert(job.id.clone(), job);
        let cache = JobCache::new(map);
        let project_path = "group/project";

        cache
            .save_with_base(project_path, Some(base_dir.clone()))
            .unwrap();

        assert!(JobCache::load_with_base(project_path, Some(base_dir.clone())).is_some());

        JobCache::clear_with_base(project_path, Some(base_dir.clone())).unwrap();

        assert!(JobCache::load_with_base(project_path, Some(base_dir)).is_none());
    }

    #[fixtura::test]
    fn test_per_project_cache_files(
        #[fixtura(id = "1".to_string(), name = "test1".to_string())] job1: GitLabJob,
        #[fixtura(id = "2".to_string(), name = "test2".to_string())] job2: GitLabJob,
    ) {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_path_buf();

        let mut map1 = HashMap::new();
        map1.insert(job1.id.clone(), job1);
        JobCache::new(map1)
            .save_with_base("group/project1", Some(base_dir.clone()))
            .unwrap();

        let mut map2 = HashMap::new();
        map2.insert(job2.id.clone(), job2);
        JobCache::new(map2)
            .save_with_base("group/project2", Some(base_dir.clone()))
            .unwrap();

        let loaded1 = JobCache::load_with_base("group/project1", Some(base_dir.clone())).unwrap();
        assert_eq!(loaded1.jobs.len(), 1);
        assert_eq!(loaded1.jobs.get("1").unwrap().name, "test1");

        let loaded2 = JobCache::load_with_base("group/project2", Some(base_dir)).unwrap();
        assert_eq!(loaded2.jobs.len(), 1);
        assert_eq!(loaded2.jobs.get("2").unwrap().name, "test2");
    }
}
