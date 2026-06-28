//! Experience persistence layer for the Experience Engine.
//!
//! Stores skills as JSON files and run stats in a simple key-value structure.
//!
//! ## Skill identity & deduplication
//!
//! Skill IDs are **stable and semantic**: derived from `trigger + scope` via
//! [`stable_skill_id`]. The same problem on the same board always maps to the
//! same file, so [`ExperienceStore::save_skill`] uses upsert semantics —
//! existing skills are refined (iteration count bumped) rather than duplicated.
//!
//! ## Relevance ranking
//!
//! [`ExperienceStore::get_skills_for`] ranks skills by:
//! 1. Trigger keyword overlap with the test description (highest weight)
//! 2. Scope specificity (board_id > exact scope > wildcard)
//! 3. Recency (newer updates rank higher, with 90-day decay)
//! 4. Hit count (popular skills get a small boost)
//!
//! [`stable_skill_id`]: stable_skill_id

use super::engine::{ExperienceRecord, RunStats, SaveOutcome};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use tracing::warn;

/// Maximum length of a single slug component in a skill ID.
/// Keeps filenames readable on all platforms (Windows MAX_PATH = 260).
const MAX_SLUG_LEN: usize = 48;

/// Generate a stable, human-readable skill ID from `trigger + scope`.
///
/// Format: `{trigger_slug}__{scope_slug}` (slugs are lowercased, non-alphanumeric
/// runs collapsed to `_`). If a slug exceeds [`MAX_SLUG_LEN`], it is truncated
/// and a short hash suffix is appended to preserve uniqueness.
///
/// Same `trigger + scope` always produces the same ID, enabling deduplication
/// and iterative refinement. The ID doubles as the JSON filename, so humans
/// can identify a skill at a glance (e.g. `closed_loop_jtag_failed__esp32s3.json`).
pub fn stable_skill_id(trigger: &str, scope: &str) -> String {
    let trigger_slug = slugify(trigger);
    let scope_slug = slugify(scope);

    // Empty trigger: fall back to a hash so we still get a stable ID.
    let trigger_part = if trigger_slug.is_empty() {
        format!("skill_{}", short_hash(trigger))
    } else if trigger_slug.len() > MAX_SLUG_LEN {
        let truncation_point = char_boundary_safe(&trigger_slug, MAX_SLUG_LEN - 9);
        format!(
            "{}_{}",
            &trigger_slug[..truncation_point],
            &short_hash(trigger)[..8]
        )
    } else {
        trigger_slug
    };

    if scope_slug.is_empty() {
        trigger_part
    } else if scope_slug.len() > MAX_SLUG_LEN {
        let truncation_point = char_boundary_safe(&scope_slug, MAX_SLUG_LEN - 9);
        format!(
            "{}__{}_{}",
            trigger_part,
            &scope_slug[..truncation_point],
            &short_hash(scope)[..8]
        )
    } else {
        format!("{}__{}", trigger_part, scope_slug)
    }
}

/// Convert a string into a URL/filename-safe slug.
///
/// - Lowercases ASCII
/// - Collapses runs of non-alphanumeric characters into a single `_`
/// - Trims leading/trailing `_`
fn slugify(s: &str) -> String {
    let lowered = s.to_lowercase();
    let mut result = String::with_capacity(lowered.len());
    let mut prev_sep = false;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
            prev_sep = false;
        } else if !prev_sep && !result.is_empty() {
            result.push('_');
            prev_sep = true;
        }
    }
    while result.ends_with('_') {
        result.pop();
    }
    result
}

/// Compute a short hex hash of an arbitrary string (for uniqueness suffixes).
fn short_hash(s: &str) -> String {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Find a safe char boundary at or before `max_bytes` in a UTF-8 string.
/// Avoids slicing mid-codepoint (which would panic).
fn char_boundary_safe(s: &str, max_bytes: usize) -> usize {
    if s.len() <= max_bytes {
        return s.len();
    }
    let mut i = max_bytes;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Sanitize a user-supplied identifier into a safe file name component.
///
/// Strips path separators, parent-dir sequences, and other characters that
/// could allow path traversal (`..`, `/`, `\`, NUL). Returns a non-empty
/// fallback (`_`) if the result would be empty.
fn sanitize_id(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Reject pure dot-sequences and empty results
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '_' || c == '.') {
        "_".to_string()
    } else {
        cleaned
    }
}

/// Compute the number of days between an RFC3339 timestamp and now.
/// Returns `None` if the timestamp cannot be parsed.
fn days_since(timestamp: &str) -> Option<u64> {
    if timestamp.is_empty() {
        return None;
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(timestamp).ok()?;
    let now = chrono::Utc::now();
    now.signed_duration_since(parsed.with_timezone(&chrono::Utc))
        .num_days()
        .max(0)
        .try_into()
        .ok()
}

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
        self.skills_dir().join(format!("{}.json", sanitize_id(id)))
    }

    fn stats_dir(&self) -> PathBuf {
        self.base_dir.join("stats")
    }

    fn stats_path(&self, board: &str, test: &str) -> PathBuf {
        self.stats_dir()
            .join(format!("{}__{}.json", sanitize_id(board), sanitize_id(test)))
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

    /// Get skills relevant to a board:test pair, **ranked by relevance**.
    ///
    /// Ranking factors (highest to lowest weight):
    /// 1. Trigger keyword overlap with `test` (exact substring match: +50, per token: +10)
    /// 2. Scope specificity (`board_id` match: +30, exact scope: +20, partial: +10)
    /// 3. Recency bonus (linear decay over 90 days, up to +18)
    /// 4. Hit count (capped at +20)
    ///
    /// Ties are broken by `last_updated` descending (newer first).
    pub fn get_skills_for(&self, board: &str, test: &str) -> Vec<ExperienceRecord> {
        let skills_dir = self.skills_dir();
        if !skills_dir.exists() {
            return vec![];
        }

        let entries = match fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    "Experience: failed to read skills dir {:?}: {}",
                    skills_dir, e
                );
                return vec![];
            }
        };

        let test_lower = test.to_lowercase();
        let test_tokens: Vec<&str> = test_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() >= 2)
            .collect();

        let mut scored: Vec<(ExperienceRecord, u32)> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Experience: failed to read skill {:?}: {}", path, e);
                    continue;
                }
            };
            let mut skill = match serde_json::from_str::<ExperienceRecord>(&content) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Experience: failed to parse skill {:?}: {}", path, e);
                    continue;
                }
            };

            // Backfill last_updated for legacy records (pre-iteration field).
            if skill.last_updated.is_empty() {
                skill.last_updated = skill.timestamp.clone();
            }

            // Scope filter
            let scope_match = skill.scope.contains(board)
                || skill.board_id.as_deref() == Some(board)
                || skill.scope == "all"
                || skill.scope == "global";
            if !scope_match {
                continue;
            }

            // Relevance score
            let trigger_lower = skill.trigger.to_lowercase();
            let mut score: u32 = 0;

            // 1. Trigger keyword match
            if !test_lower.is_empty() && trigger_lower.contains(&test_lower) {
                score += 50; // Exact substring match
            }
            for token in &test_tokens {
                if trigger_lower.contains(token) {
                    score += 10;
                }
            }

            // 2. Scope specificity bonus
            if skill.board_id.as_deref() == Some(board) {
                score += 30;
            } else if skill.scope == board {
                score += 20;
            } else if skill.scope != "all" && skill.scope != "global" {
                score += 10;
            }

            // 3. Recency bonus (90-day linear decay)
            if let Some(days) = days_since(&skill.last_updated) {
                if days < 90 {
                    score += ((90 - days) / 5) as u32; // Up to +18
                }
            }

            // 4. Hit count bonus (capped)
            score += skill.hit_count.min(20);

            scored.push((skill, score));
        }

        // Sort by score descending; ties broken by last_updated descending.
        scored.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.0.last_updated.cmp(&a.0.last_updated))
        });

        scored.into_iter().map(|(s, _)| s).collect()
    }

    /// Save a skill with **upsert semantics**.
    ///
    /// If a skill with the same ID already exists:
    /// - `fix` is replaced with the new value
    /// - `lesson` is replaced unless the new value is empty
    /// - `source_ref` is replaced if the new value is `Some`, else kept
    /// - `last_updated` is set to now
    /// - `iterations` is bumped by 1
    /// - `hit_count`, `last_hit`, `timestamp` are preserved
    ///
    /// If no existing skill: a new record is written as-is.
    pub fn save_skill(&self, record: &ExperienceRecord) -> SaveOutcome {
        let dir = self.skills_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            warn!("Experience: failed to create skills dir {:?}: {}", dir, e);
            return SaveOutcome::Failed;
        }
        let path = self.skills_path(&record.id);

        // Try to load existing record (upsert path).
        if let Ok(existing_content) = fs::read_to_string(&path) {
            if let Ok(mut existing) = serde_json::from_str::<ExperienceRecord>(&existing_content) {
                // Merge: preserve identity & history, update fix/lesson.
                existing.fix = record.fix.clone();
                if !record.lesson.is_empty() {
                    existing.lesson = record.lesson.clone();
                }
                existing.source_ref = record
                    .source_ref
                    .clone()
                    .or(existing.source_ref.take());
                existing.last_updated = if record.last_updated.is_empty() {
                    chrono::Utc::now().to_rfc3339()
                } else {
                    record.last_updated.clone()
                };
                existing.iterations = existing.iterations.saturating_add(1);

                return match serde_json::to_string_pretty(&existing) {
                    Ok(content) => match fs::write(&path, content) {
                        Ok(_) => SaveOutcome::Updated {
                            iterations: existing.iterations,
                        },
                        Err(e) => {
                            warn!(
                                "Experience: failed to write updated skill {}: {}",
                                path.display(),
                                e
                            );
                            SaveOutcome::Failed
                        }
                    },
                    Err(e) => {
                        warn!(
                            "Experience: failed to serialize updated skill {}: {}",
                            record.id,
                            e
                        );
                        SaveOutcome::Failed
                    }
                };
            }
            // Existing file was unparseable — fall through to create path,
            // which will overwrite the corrupted file.
            warn!(
                "Experience: existing skill {:?} was unparseable, overwriting",
                path
            );
        }

        // New skill path.
        match serde_json::to_string_pretty(record) {
            Ok(content) => match fs::write(&path, content) {
                Ok(_) => SaveOutcome::Created,
                Err(e) => {
                    warn!(
                        "Experience: failed to write new skill {}: {}",
                        path.display(),
                        e
                    );
                    SaveOutcome::Failed
                }
            },
            Err(e) => {
                warn!(
                    "Experience: failed to serialize new skill {}: {}",
                    record.id,
                    e
                );
                SaveOutcome::Failed
            }
        }
    }

    /// Bump the `hit_count` and update `last_hit` for a skill (best-effort).
    ///
    /// Called when a skill is retrieved as relevant. Failures are logged but
    /// do not propagate — hit tracking is a ranking hint, not a critical path.
    pub fn mark_skill_hit(&self, id: &str) {
        let path = self.skills_path(id);
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return, // Skill may have been deleted; silent.
        };
        let mut skill = match serde_json::from_str::<ExperienceRecord>(&content) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    "Experience: failed to parse skill {:?} for hit tracking: {}",
                    path, e
                );
                return;
            }
        };
        skill.hit_count = skill.hit_count.saturating_add(1);
        skill.last_hit = Some(chrono::Utc::now().to_rfc3339());
        if let Ok(json) = serde_json::to_string_pretty(&skill) {
            if let Err(e) = fs::write(&path, json) {
                warn!(
                    "Experience: failed to persist hit_count for {:?}: {}",
                    path, e
                );
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
                warn!(
                    "Experience: failed to read skills dir {:?}: {}",
                    skills_dir, e
                );
                return vec![];
            }
        };
        for entry in entries.flatten() {
            match fs::read_to_string(entry.path()) {
                Ok(content) => match serde_json::from_str::<ExperienceRecord>(&content) {
                    Ok(mut skill) => {
                        // Backfill last_updated for legacy records.
                        if skill.last_updated.is_empty() {
                            skill.last_updated = skill.timestamp.clone();
                        }
                        results.push(skill);
                    }
                    Err(e) => {
                        warn!(
                            "Experience: failed to parse skill {:?}: {}",
                            entry.path(),
                            e
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        "Experience: failed to read skill {:?}: {}",
                        entry.path(),
                        e
                    );
                }
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_skill_id_is_deterministic() {
        let a = stable_skill_id("closed_loop_jtag_failed", "esp32s3");
        let b = stable_skill_id("closed_loop_jtag_failed", "esp32s3");
        assert_eq!(a, b);
        assert!(a.contains("closed_loop_jtag_failed"));
        assert!(a.contains("esp32s3"));
    }

    #[test]
    fn stable_skill_id_differs_for_different_scopes() {
        let a = stable_skill_id("same_trigger", "esp32s3");
        let b = stable_skill_id("same_trigger", "esp32c3");
        assert_ne!(a, b);
    }

    #[test]
    fn stable_skill_id_handles_empty_inputs() {
        let id = stable_skill_id("", "");
        assert!(!id.is_empty());
        // Should not panic and should be deterministic.
        assert_eq!(id, stable_skill_id("", ""));
    }

    #[test]
    fn stable_skill_id_truncates_long_triggers() {
        let long_trigger = "a".repeat(200);
        let id = stable_skill_id(&long_trigger, "scope");
        // ID should be bounded in length (slug + separator + hash suffix).
        assert!(id.len() < 200);
        // Should still be deterministic.
        assert_eq!(id, stable_skill_id(&long_trigger, "scope"));
    }

    #[test]
    fn stable_skill_id_normalizes_case_and_punctuation() {
        let a = stable_skill_id("Closed-Loop JTAG Failed!", "ESP32-S3");
        let b = stable_skill_id("closed_loop_jtag_failed", "esp32_s3");
        assert_eq!(a, b);
    }

    #[test]
    fn slugify_collapses_runs() {
        assert_eq!(slugify("Hello, World!"), "hello_world");
        assert_eq!(slugify("  multiple   spaces  "), "multiple_spaces");
        assert_eq!(slugify("---leading-trailing---"), "leading_trailing");
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn char_boundary_safe_never_panics() {
        // Multi-byte UTF-8: '中' is 3 bytes.
        let s = "中文字符";
        let boundary = char_boundary_safe(s, 4); // Mid-codepoint.
        assert!(s.is_char_boundary(boundary));
        assert!(boundary <= 4);
    }

    #[test]
    fn days_since_handles_empty_and_invalid() {
        assert!(days_since("").is_none());
        assert!(days_since("not-a-date").is_none());
        // A recent timestamp should return a small number of days.
        let recent = chrono::Utc::now().to_rfc3339();
        let days = days_since(&recent).unwrap();
        assert!(days <= 1);
    }
}
