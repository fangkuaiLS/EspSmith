//! Experience Engine — experience management.

use super::storage;
use dirs_next;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A reusable engineering skill/experience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceRecord {
    /// Unique ID
    pub id: String,
    /// When this skill applies (symptom)
    pub trigger: String,
    /// Exact resolution
    pub fix: String,
    /// Reusable lesson/rule
    pub lesson: String,
    /// Applicability scope (e.g. "stm32f4", "all_esp32")
    pub scope: String,
    /// Specific board ID if narrower than scope
    pub board_id: Option<String>,
    /// Source reference (file path, commit, etc.)
    pub source_ref: Option<String>,
    /// When this was recorded
    pub timestamp: String,
}

/// Run statistics for a given board:test pair.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunStats {
    pub total_runs: u32,
    pub success_count: u32,
    pub failed_count: u32,
    /// Auto-calculated confidence (0-100)
    pub confidence: u32,
}

impl RunStats {
    #[allow(dead_code)] // 构造器预留
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_success(&mut self) {
        self.total_runs += 1;
        self.success_count += 1;
        self.recalc_confidence();
    }

    pub fn record_failure(&mut self) {
        self.total_runs += 1;
        self.failed_count += 1;
        self.recalc_confidence();
    }

    fn recalc_confidence(&mut self) {
        if self.total_runs == 0 {
            self.confidence = 0;
        } else {
            // Weighted: recent success counts more
            self.confidence = ((self.success_count as f64 / self.total_runs as f64) * 100.0) as u32;
        }
    }
}

/// Context returned before a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceContext {
    pub available: bool,
    pub board: String,
    pub test: String,
    pub run_stats: RunStats,
    pub relevant_skills: Vec<ExperienceRecord>,
    pub likely_pitfalls: Vec<String>,
    pub observation_focus: Vec<String>,
}

/// The Experience Engine.
pub struct ExperienceEngine {
    store: storage::ExperienceStore,
}

impl ExperienceEngine {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        let dir = base_dir.unwrap_or_else(|| dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("espsmith")
            .join("experience"));
        Self {
            store: storage::ExperienceStore::new(dir),
        }
    }

    /// Query context before a run.
    pub fn query_context(&self, board: &str, test: &str) -> ExperienceContext {
        let stats = self.store.get_stats(board, test);
        let skills = self.store.get_skills_for(board, test);
        let pitfalls: Vec<String> = skills
            .iter()
            .filter(|s| s.trigger.contains("危险") || s.trigger.contains("PITFALL"))
            .map(|s| format!("{} → {}", s.trigger, s.fix))
            .collect();
        let focus: Vec<String> = skills
            .iter()
            .filter(|s| s.trigger.contains("观察") || s.trigger.contains("FOCUS"))
            .map(|s| s.trigger.clone())
            .collect();

        ExperienceContext {
            available: true,
            board: board.to_string(),
            test: test.to_string(),
            run_stats: stats,
            relevant_skills: skills,
            likely_pitfalls: pitfalls,
            observation_focus: focus,
        }
    }

    /// Record a run result.
    pub fn record_run(&mut self, board: &str, test: &str, passed: bool) -> RunStats {
        self.store.record_run(board, test, passed)
    }

    /// Record a skill/experience.
    pub fn record_skill(&mut self, record: ExperienceRecord) {
        self.store.save_skill(&record);
    }

    /// List all skills for a scope.
    #[allow(dead_code)] // 技能查询预留
    pub fn list_skills(&self, scope: Option<&str>) -> Vec<ExperienceRecord> {
        self.store.list_skills().into_iter()
            .filter(|s| scope.map_or(true, |sc| s.scope.contains(sc)))
            .collect()
    }
}

impl Default for ExperienceEngine {
    fn default() -> Self {
        Self::new(None)
    }
}
