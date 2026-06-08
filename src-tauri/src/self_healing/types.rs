//! Core data structures for the AEL-inspired closed-loop Self-Healing engine.
//!
//! Mirrors the key concepts from AEL's runner.py and pipeline.py:
//! - Step:     A single executable stage in a run plan
//! - Plan:     Ordered list of steps with a recovery policy
//! - RunResult:Per-step and overall pass/fail with structured output
//! - RecoveryPolicy: Controls retry budgets and anchor points
//! - RunContext: Context passed through the pipeline

use serde::{Deserialize, Serialize};

/// Execution status of a step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Running,
    Passed,
    Failed,
    Skipped,
}

/// Recovery action to take when a step fails all retries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Do nothing, let Self-Healing continue
    None,
    /// Serial port reset
    SerialReset,
    /// Probe soft reset (e.g., OpenOCD soft_reset_halt)
    ProbeSoftReset,
    /// Probe hard reset
    ProbeHardReset,
    /// Power cycle the DUT
    PowerCycle,
    /// Custom action with description
    Custom(String),
}

/// Recovery hint: what action to take and where to rewind to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryHint {
    pub action: RecoveryAction,
    /// Anchor to rewind to: "build", "load", or "check" (current step)
    pub anchor: AnchorPoint,
    /// Human-readable description
    pub reason: String,
}

/// Anchor point for Self-Healing rewind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnchorPoint {
    /// Rewind to the first step
    Start,
    /// Rewind to the build step
    Build,
    /// Rewind to the load/flash step
    Load,
    /// Rewind to the current check/verify step
    Check,
}

/// Retry budget for categories of steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryBudget {
    /// Retries for build steps
    pub build: u32,
    /// Retries for run/load steps (flash)
    pub run: u32,
    /// Retries for check/verify steps
    pub check: u32,
}

impl Default for RetryBudget {
    fn default() -> Self {
        Self { build: 1, run: 2, check: 2 }
    }
}

/// Recovery policy for a run plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    pub enabled: bool,
    /// Allowed recovery actions
    pub allowed_actions: Vec<RecoveryAction>,
    /// Retry budgets per step category
    pub retries: RetryBudget,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_actions: vec![
                RecoveryAction::SerialReset,
                RecoveryAction::ProbeSoftReset,
            ],
            retries: RetryBudget::default(),
        }
    }
}

impl RecoveryPolicy {
    pub fn full() -> Self {
        Self {
            enabled: true,
            allowed_actions: vec![
                RecoveryAction::SerialReset,
                RecoveryAction::ProbeSoftReset,
                RecoveryAction::ProbeHardReset,
            ],
            retries: RetryBudget::default(),
        }
    }
}

/// Category of a Self-Healing step (determines which retry budget to use).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepCategory {
    Build,
    Load,
    Check,
}

/// A single step in a run plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub name: String,
    pub category: StepCategory,
    pub description: String,
    /// The adapter to execute (e.g., "build.cmake", "flash.gdbmi")
    pub adapter: String,
    /// Parameters for this step
    #[serde(default)]
    pub params: serde_json::Value,
    /// Timeout for this step
    pub timeout_s: Option<f64>,
}

/// A run plan: ordered list of steps with a recovery policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub name: String,
    pub board: String,
    pub test: String,
    pub steps: Vec<Step>,
    pub recovery_policy: RecoveryPolicy,
    /// Overall timeout in seconds
    pub timeout_s: Option<f64>,
    /// Safety: maximum total step executions before giving up
    pub guard_limit: Option<usize>,
}

/// Result of a single step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_name: String,
    pub status: StepStatus,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub duration_ms: u64,
    pub attempt: u32,
    pub error: Option<String>,
}

impl StepResult {
    pub fn passed(name: &str, attempt: u32, duration_ms: u64) -> Self {
        Self {
            step_name: name.to_string(),
            status: StepStatus::Passed,
            exit_code: Some(0),
            stdout: None,
            stderr: None,
            duration_ms,
            attempt,
            error: None,
        }
    }

    pub fn failed(name: &str, attempt: u32, error: String, duration_ms: u64) -> Self {
        Self {
            step_name: name.to_string(),
            status: StepStatus::Failed,
            exit_code: Some(1),
            stdout: None,
            stderr: None,
            duration_ms,
            attempt,
            error: Some(error),
        }
    }
}

/// Overall run result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub plan_name: String,
    pub board: String,
    pub test: String,
    pub passed: bool,
    pub total_steps: usize,
    pub passed_steps: usize,
    pub step_results: Vec<StepResult>,
    pub total_duration_ms: u64,
    pub total_attempts: usize,
    pub recovery_applied: Vec<String>,
    pub summary: String,
}

impl RunResult {
    pub fn new(plan: &Plan) -> Self {
        Self {
            plan_name: plan.name.clone(),
            board: plan.board.clone(),
            test: plan.test.clone(),
            passed: false,
            total_steps: plan.steps.len(),
            passed_steps: 0,
            step_results: Vec::new(),
            total_duration_ms: 0,
            total_attempts: 0,
            recovery_applied: Vec::new(),
            summary: String::new(),
        }
    }
}

/// Intermediate state emitted by the Self-Healing runner while it is still running.
/// The frontend uses these to display "第 N 次尝试 / 已应用 X 恢复操作" under
/// the active step in the OperationTimeline so the user can tell apart
/// "still doing GDB PC/堆栈检查" from "third ProbeSoftReset retry".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RunnerEvent {
    /// A step is about to be attempted. `attempt` is 1-based and counts the
    /// current retry within the step's budget.
    StepStarted {
        step_index: usize,
        step_name: String,
        attempt: u32,
        total_attempts: usize,
    },
    /// A step attempt has failed and the runner is going to either retry
    /// within budget, or fall through to recovery.
    StepFailed {
        step_index: usize,
        step_name: String,
        attempt: u32,
        total_attempts: usize,
        error: String,
        will_retry: bool,
    },
    /// A step passed.
    StepPassed {
        step_index: usize,
        step_name: String,
        attempt: u32,
        total_attempts: usize,
        duration_ms: u64,
    },
    /// The runner resolved a recovery hint and is executing it. `rewind_to`
    /// is the step index the Self-Healing engine will restart from, or `None` if the
    /// runner continues with the next step.
    RecoveryApplied {
        step_index: usize,
        step_name: String,
        action: String,
        reason: String,
        rewind_to: Option<usize>,
    },
}