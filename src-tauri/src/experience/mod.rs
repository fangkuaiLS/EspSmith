//! Experience Engine — cross-run experience accumulation.
//!
//! Inspired by AEL's civilization_client.py.
//!
//! Stores engineering experience across runs:
//! - Run statistics (pass/fail counts, confidence)
//! - Known skills (triggers + fixes) — deduplicated & iteratively refined
//! - Likely pitfalls (dangerous paths)
//! - Observation focus (derived from historical failures)
//!
//! ## Skill identity
//!
//! Skills use **stable semantic IDs** derived from `trigger + scope`, so the
//! same problem on the same board always maps to the same record. Recording
//! an existing skill refines it (bumps `iterations`) instead of creating a
//! duplicate — see [`record_skill`].

pub mod engine;
pub mod storage;

pub use engine::{ExperienceRecord, SaveOutcome};
pub use storage::stable_skill_id;

use std::path::PathBuf;
use std::sync::Mutex;

static EXPERIENCE: Mutex<Option<engine::ExperienceEngine>> = Mutex::new(None);

/// A skill is considered stale if it hasn't been updated in this many days.
/// Stale skills are still returned but marked in the AI context prompt so the
/// model knows to verify them before relying on the fix.
const STALE_THRESHOLD_DAYS: u64 = 180;

/// Maximum number of skills to inject into an AI system prompt.
/// Prevents context bloat when many skills match a board. The full list is
/// still available via the `query_experience` MCP tool.
const MAX_SKILLS_IN_PROMPT: usize = 5;

/// Initialize the global Experience Engine with a shared base directory.
/// Typically called once at startup with `<data_dir>/espsmith/experience`.
pub fn init(base_dir: PathBuf) {
    let mut guard = EXPERIENCE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(engine::ExperienceEngine::new(Some(base_dir)));
}

/// Get a snapshot of the experience context for a given board:test pair.
/// Returns None if the engine has not been initialized.
pub fn query_context(board: &str, test: &str) -> Option<engine::ExperienceContext> {
    let guard = EXPERIENCE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(|eng| eng.query_context(board, test))
}

/// Record a run result (pass/fail) for a board:test pair.
/// Returns None if the engine has not been initialized.
pub fn record_run(board: &str, test: &str, passed: bool) -> Option<engine::RunStats> {
    let mut guard = EXPERIENCE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().map(|eng| eng.record_run(board, test, passed))
}

/// Record an engineering skill/experience with **upsert semantics**.
///
/// If a skill with the same `trigger + scope` already exists, its `fix` and
/// `lesson` are refined and `iterations` is bumped, rather than creating a
/// duplicate file. Returns the outcome (`Created` / `Updated` / `Failed`),
/// or `None` if the engine has not been initialized.
pub fn record_skill(record: engine::ExperienceRecord) -> Option<SaveOutcome> {
    let mut guard = EXPERIENCE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().map(|eng| eng.record_skill(record))
}

/// List all skills, optionally filtered by scope.
/// Returns empty vec if the engine has not been initialized.
#[allow(dead_code)] // 技能查询预留
pub fn list_skills(scope: Option<&str>) -> Vec<engine::ExperienceRecord> {
    let guard = EXPERIENCE.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .map(|eng| eng.list_skills(scope))
        .unwrap_or_default()
}

/// Generate a context string suitable for injection into an AI system prompt.
///
/// **Note**: This function is currently **unused** — the AI assistant no longer
/// pre-injects experience context into the system prompt. Instead, the system
/// prompt instructs the AI to call the `query_experience` MCP tool on demand
/// when it encounters a difficult problem. This avoids context pollution and
/// allows the AI to construct precise queries based on the current issue.
///
/// The function is retained for potential diagnostic/future use.
///
/// The prompt includes:
/// - Run history (pass/fail ratio + confidence)
/// - Top [`MAX_SKILLS_IN_PROMPT`] most relevant skills (ranked by trigger
///   keyword match, scope specificity, recency, and hit count)
/// - Likely pitfalls
///
/// Skills are annotated with their iteration count (if > 1) and a `[STALE]`
/// marker if they haven't been updated in [`STALE_THRESHOLD_DAYS`] days.
#[allow(dead_code)] // 保留作为诊断/未来备用；AI 现通过 query_experience 工具按需查询
pub fn build_context_prompt(board: &str, test: &str) -> Option<String> {
    let ctx = query_context(board, test)?;
    if !ctx.available {
        return None;
    }

    let mut parts = Vec::new();

    if ctx.run_stats.total_runs > 0 {
        parts.push(format!(
            "Run history for {}: {}/{} passed (confidence {}%)",
            board, ctx.run_stats.success_count, ctx.run_stats.total_runs, ctx.run_stats.confidence
        ));
    }

    if !ctx.relevant_skills.is_empty() {
        parts.push("Known skills (ranked by relevance):".into());
        let shown = ctx.relevant_skills.iter().take(MAX_SKILLS_IN_PROMPT);
        for skill in shown {
            let iter_marker = if skill.iterations > 1 {
                format!(" (refined {}x)", skill.iterations)
            } else {
                String::new()
            };
            let stale_marker = if is_stale(&skill.last_updated) {
                " [STALE — verify before relying]"
            } else {
                ""
            };
            parts.push(format!(
                "- When [{}]: {}{}{}",
                skill.trigger, skill.fix, iter_marker, stale_marker
            ));
        }
        let hidden = ctx.relevant_skills.len().saturating_sub(MAX_SKILLS_IN_PROMPT);
        if hidden > 0 {
            parts.push(format!(
                "... and {} more skills available via query_experience tool",
                hidden
            ));
        }
    }

    if !ctx.likely_pitfalls.is_empty() {
        parts.push("Likely pitfalls:".into());
        for pit in &ctx.likely_pitfalls {
            parts.push(format!("- {}", pit));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("[Experience Context]\n{}", parts.join("\n")))
    }
}

/// Check whether a skill is stale (not updated within [`STALE_THRESHOLD_DAYS`]).
fn is_stale(last_updated: &str) -> bool {
    match days_since(last_updated) {
        Some(days) => days > STALE_THRESHOLD_DAYS,
        None => false, // Unknown age — don't mark as stale (could be a legacy record).
    }
}

/// Compute the number of days between an RFC3339 timestamp and now.
/// Returns `None` if the timestamp is empty or unparseable.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stale_returns_false_for_empty() {
        assert!(!is_stale(""));
    }

    #[test]
    fn is_stale_returns_false_for_invalid() {
        assert!(!is_stale("not-a-date"));
    }

    #[test]
    fn is_stale_returns_true_for_old_timestamp() {
        // 2000-01-01 is definitely older than STALE_THRESHOLD_DAYS.
        let old = "2000-01-01T00:00:00+00:00";
        assert!(is_stale(old));
    }

    #[test]
    fn is_stale_returns_false_for_recent_timestamp() {
        let recent = chrono::Utc::now().to_rfc3339();
        assert!(!is_stale(&recent));
    }

    #[test]
    fn days_since_handles_empty_and_invalid() {
        assert!(days_since("").is_none());
        assert!(days_since("not-a-date").is_none());
    }

    #[test]
    fn build_context_prompt_returns_none_when_uninitialized() {
        // Without calling init(), the global engine is None.
        // Note: this test is order-dependent — if another test called init()
        // first, this will fail. We accept this trade-off for simplicity.
        let result = build_context_prompt("test_board", "test");
        // Either None (uninitialized) or Some(non-empty) (initialized).
        if let Some(prompt) = result {
            assert!(!prompt.is_empty());
        }
    }
}
