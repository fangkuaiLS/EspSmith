//! Experience Engine — experience management.

use super::storage;
use dirs_next;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A reusable engineering skill/experience.
///
/// Records use **stable semantic IDs** derived from `trigger + scope`, so the
/// same problem on the same board always maps to the same record. This enables
/// deduplication and iterative refinement instead of accumulating duplicates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceRecord {
    /// Stable semantic ID: `{trigger_slug}__{scope_slug}` (truncated + hash
    /// suffix if too long). Same trigger+scope always produces the same ID.
    pub id: String,
    /// When this skill applies (symptom / trigger condition).
    pub trigger: String,
    /// Exact resolution that worked.
    pub fix: String,
    /// Reusable lesson/rule (the "why").
    pub lesson: String,
    /// Applicability scope (e.g. "esp32s3", "all_esp32", "global").
    pub scope: String,
    /// Specific board ID if narrower than scope.
    pub board_id: Option<String>,
    /// Source reference (file path, commit, etc.).
    pub source_ref: Option<String>,
    /// When this skill was first recorded (RFC3339).
    pub timestamp: String,
    /// When this skill was last updated (RFC3339). Used for staleness check.
    /// Backfilled from `timestamp` when loading legacy records.
    #[serde(default)]
    pub last_updated: String,
    /// How many times this skill has been retrieved as relevant.
    /// Used as a ranking signal (popular skills rank higher).
    #[serde(default)]
    pub hit_count: u32,
    /// When this skill was last retrieved (None if never hit).
    #[serde(default)]
    pub last_hit: Option<String>,
    /// Iteration count — how many times the fix has been refined via upsert.
    /// Starts at 1 for a newly created skill.
    #[serde(default = "default_iterations")]
    pub iterations: u32,
}

fn default_iterations() -> u32 {
    1
}

/// Outcome of saving a skill (upsert semantics).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveOutcome {
    /// A new skill file was created.
    Created,
    /// An existing skill was updated (fix/lesson refined, iteration bumped).
    Updated {
        /// New iteration count after this update.
        iterations: u32,
    },
    /// Persistence failed (details logged via tracing).
    Failed,
}

impl SaveOutcome {
    #[allow(dead_code)] // 公共 API 预留，供调用方判断是否成功
    pub fn is_success(&self) -> bool {
        !matches!(self, SaveOutcome::Failed)
    }
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
    /// Skills ranked by relevance to the current board:test pair (most relevant first).
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
        let dir = base_dir.unwrap_or_else(|| {
            dirs_next::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("espsmith")
                .join("experience")
        });
        Self {
            store: storage::ExperienceStore::new(dir),
        }
    }

    /// Query context before a run. Side effect: bumps `hit_count` for the
    /// top-ranked skills (capped to avoid write amplification).
    pub fn query_context(&self, board: &str, test: &str) -> ExperienceContext {
        let stats = self.store.get_stats(board, test);
        let skills = self.store.get_skills_for(board, test);

        // Bump hit_count for the top 3 most relevant skills (best-effort,
        // failures are logged but do not affect the query result).
        const HIT_TRACKING_TOP_N: usize = 3;
        for skill in skills.iter().take(HIT_TRACKING_TOP_N) {
            self.store.mark_skill_hit(&skill.id);
        }

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

    /// Record a skill/experience. Uses **upsert semantics**: if a skill with
    /// the same `trigger + scope` already exists, its fix/lesson are refined
    /// and the iteration count is bumped, rather than creating a duplicate.
    pub fn record_skill(&mut self, record: ExperienceRecord) -> SaveOutcome {
        self.store.save_skill(&record)
    }

    /// List all skills, optionally filtered by scope.
    #[allow(dead_code)] // 技能查询预留
    pub fn list_skills(&self, scope: Option<&str>) -> Vec<ExperienceRecord> {
        self.store
            .list_skills()
            .into_iter()
            .filter(|s| scope.map_or(true, |sc| s.scope.contains(sc)))
            .collect()
    }
}

impl Default for ExperienceEngine {
    fn default() -> Self {
        Self::new(None)
    }
}
