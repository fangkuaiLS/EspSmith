//! Experience persistence layer for the Experience Engine.
//!
//! Stores skills as JSON files and run stats in a simple key-value structure.

use super::engine::{ExperienceRecord, RunStats};
use std::fs;
use std::path::PathBuf;
use tracing::warn;

/// Simple file-based experience store.
pub struct ExperienceStore {
    base_dir: PathBuf,
}

impl ExperienceStore {
    pub fn new(base_dir: PathBuf) -> Self {
        if let Err(e) = fs::create_dir_all(&base_dir) {
            warn!("Experience: failed to create base dir {:?}: {}", base_dir, e);
        }
        Self { base_dir }
    }

    fn skills_dir(&self) -> PathBuf {
        self.base_dir.join("skills")
    }

    fn skills_path(&self, id: &str) -> PathBuf {
        self.skills_dir().join(format!("{id}.json"))
    }

    fn stats_dir(&self) -> PathBuf {
        self.base_dir.join("stats")
    }

    fn stats_path(&self, board: &str, test: &str) -> PathBuf {
        self.stats_dir().join(format!("{board}__{test}.json"))
    }

    /// Get run statistics for a board:test pair.
    pub fn get_stats(&self, board: &str, test: &str) -> RunStats {
        let path = self.stats_path(board, test);
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => RunStats::default(),
        }
    }

    /// Record a run result.
    pub fn record_run(&mut self, board: &str, test: &str, passed: bool) -> RunStats {
        let mut stats = self.get_stats(board, test);
        if passed {
            stats.record_success();
        } else {
            stats.record_failure();
        }
        let path = self.stats_path(board, test);
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!("Experience: failed to create stats dir {:?}: {}", parent, e);
            }
        }
        match serde_json::to_string_pretty(&stats) {
            Ok(content) => {
                if let Err(e) = fs::write(&path, content) {
                    warn!("Experience: failed to write stats {}: {}", path.display(), e);
                }
            }
            Err(e) => {
                warn!("Experience: failed to serialize stats: {}", e);
            }
        }
        stats
    }

    /// Get skills relevant to a board:test pair.
    pub fn get_skills_for(&self, board: &str, _test: &str) -> Vec<ExperienceRecord> {
        let skills_dir = self.skills_dir();
        if !skills_dir.exists() {
            return vec![];
        }

        let mut results = Vec::new();
        let entries = match fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!("Experience: failed to read skills dir {:?}: {}", skills_dir, e);
                return vec![];
            }
        };
        for entry in entries.flatten() {
            match fs::read_to_string(entry.path()) {
                Ok(content) => {
                    match serde_json::from_str::<ExperienceRecord>(&content) {
                        Ok(skill) => {
                            if skill.scope.contains(board)
                                || skill.board_id.as_deref() == Some(board)
                                || skill.scope == "all"
                                || skill.scope == "global"
                            {
                                results.push(skill);
                            }
                        }
                        Err(e) => {
                            warn!("Experience: failed to parse skill {:?}: {}", entry.path(), e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Experience: failed to read skill {:?}: {}", entry.path(), e);
                }
            }
        }
        results
    }

    /// Save a new skill.
    pub fn save_skill(&self, record: &ExperienceRecord) {
        let dir = self.skills_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            warn!("Experience: failed to create skills dir {:?}: {}", dir, e);
        }
        let path = self.skills_path(&record.id);
        match serde_json::to_string_pretty(record) {
            Ok(content) => {
                if let Err(e) = fs::write(&path, content) {
                    warn!("Experience: failed to write skill {}: {}", path.display(), e);
                }
            }
            Err(e) => {
                warn!("Experience: failed to serialize skill {}: {}", record.id, e);
            }
        }
    }

    /// List all stored skills.
    #[allow(dead_code)] // 技能查询预留
    pub fn list_skills(&self) -> Vec<ExperienceRecord> {
        let skills_dir = self.skills_dir();
        if !skills_dir.exists() {
            return vec![];
        }

        let mut results = Vec::new();
        let entries = match fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!("Experience: failed to read skills dir {:?}: {}", skills_dir, e);
                return vec![];
            }
        };
        for entry in entries.flatten() {
            match fs::read_to_string(entry.path()) {
                Ok(content) => {
                    match serde_json::from_str::<ExperienceRecord>(&content) {
                        Ok(skill) => results.push(skill),
                        Err(e) => {
                            warn!("Experience: failed to parse skill {:?}: {}", entry.path(), e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Experience: failed to read skill {:?}: {}", entry.path(), e);
                }
            }
        }
        results
    }
}
