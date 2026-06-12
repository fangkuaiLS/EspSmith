//! Experience Engine — cross-run experience accumulation.
//!
//! Inspired by AEL's civilization_client.py.
//!
//! Stores engineering experience across runs:
//! - Run statistics (pass/fail counts, confidence)
//! - Known skills (triggers + fixes)
//! - Likely pitfalls (dangerous paths)
//! - Observation focus (derived from historical failures)

pub mod engine;
pub mod storage;

pub use engine::ExperienceRecord;

use std::path::PathBuf;
use std::sync::Mutex;

static EXPERIENCE: Mutex<Option<engine::ExperienceEngine>> = Mutex::new(None);

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

/// Record an engineering skill/experience.
/// Returns false if the engine has not been initialized.
pub fn record_skill(record: engine::ExperienceRecord) -> bool {
    let mut guard = EXPERIENCE.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_mut() {
        Some(eng) => { eng.record_skill(record); true }
        None => false,
    }
}

/// List all skills, optionally filtered by scope.
/// Returns empty vec if the engine has not been initialized.
#[allow(dead_code)] // 技能查询预留
pub fn list_skills(scope: Option<&str>) -> Vec<engine::ExperienceRecord> {
    let guard = EXPERIENCE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().map(|eng| eng.list_skills(scope)).unwrap_or_default()
}

/// Generate a context string suitable for injection into an AI system prompt.
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
        parts.push("Known skills:".into());
        for skill in &ctx.relevant_skills {
            parts.push(format!("- When [{}]: {}", skill.trigger, skill.fix));
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
