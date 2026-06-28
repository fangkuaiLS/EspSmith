//! Self-Healing runner — the core retry + rewind + recovery loop.
//!
//! Inspired by AEL's runner.py `run_plan()`:
//!  1. Execute each step
//!  2. On failure: retry with budget
//!  3. Exhausted budget: resolve recovery hint
//!  4. Execute recovery action
//!  5. Rewind to anchor point and retry
//!  6. Safety: timeout + guard_limit

use super::types::*;
use super::recovery;
use std::time::{Duration, Instant};

/// Lightweight `Fn` alias for runner events. Implementations typically forward
/// to a Tauri event so the frontend can show "第 N 次尝试 / 已应用 X 恢复操作".
pub type RunnerEventSink = dyn Fn(&RunnerEvent) + Send + Sync;

/// Run a plan without emitting any intermediate events.
#[allow(dead_code, clippy::type_complexity)] // Self-Healing直接执行预留
pub fn run_plan(
    plan: &Plan,
    execute_fn: &dyn Fn(&Step, &mut std::collections::HashMap<String, String>) -> Result<StepResult, String>,
) -> RunResult {
    run_plan_with_progress(plan, execute_fn, &|_| {})
}

/// Execute a full run plan with retry, recovery, and rewind, calling `on_event`
/// at every meaningful transition (step started / failed / passed, recovery
/// applied). The sink is invoked synchronously from the runner thread; the
/// implementation should be cheap and non-blocking (e.g. emit a Tauri event).
#[allow(clippy::type_complexity)]
pub fn run_plan_with_progress(
    plan: &Plan,
    execute_fn: &dyn Fn(&Step, &mut std::collections::HashMap<String, String>) -> Result<StepResult, String>,
    on_event: &RunnerEventSink,
) -> RunResult {
    let mut result = RunResult::new(plan);
    let start_time = Instant::now();
    let guard_limit = plan.guard_limit.unwrap_or(100);
    let overall_timeout = plan.timeout_s.map(Duration::from_secs_f64);

    // Track per-step retry budgets
    let mut budget_map: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    let mut step_index: usize = 0;
    let mut total_executions: usize = 0;
    let mut ctx = std::collections::HashMap::new();

    while step_index < plan.steps.len() {
        // Safety checks
        if total_executions >= guard_limit {
            result.summary = format!("Guard limit ({guard_limit}) exceeded. Self-Healing aborted.");
            return result;
        }
        if let Some(timeout) = overall_timeout {
            if start_time.elapsed() >= timeout {
                result.summary = format!("Overall timeout ({:.0}s) exceeded.", timeout.as_secs());
                return result;
            }
        }

        let step = &plan.steps[step_index];
        let budget = budget_map.entry(step_index).or_insert_with(|| {
            match step.category {
                StepCategory::Build => plan.recovery_policy.retries.build,
                StepCategory::Load => plan.recovery_policy.retries.run,
                StepCategory::Check => plan.recovery_policy.retries.check,
            }
        });

        let mut step_fatal = false;

        // Attempt execution
        let step_start = Instant::now();
        let mut step_result: Option<StepResult> = None;
        // Per-iteration error for the StepFailed event. None if the attempt
        // ultimately passed.
        let mut last_attempt_error: Option<String> = None;
        #[allow(unused_assignments)]
        let mut last_attempt_will_retry = false;

        for attempt in 0..=*budget {
            total_executions += 1;
            tracing::info!(
                "[Self-Healing] StepStarted: index={}, name={}, attempt={}/{}",
                step_index,
                step.name,
                attempt + 1,
                *budget + 1
            );
            on_event(&RunnerEvent::StepStarted {
                step_index,
                step_name: step.name.clone(),
                attempt: attempt + 1,
                total_attempts: total_executions,
            });

            match execute_fn(step, &mut ctx) {
                Ok(sr) => {
                    let mut sr = sr;
                    sr.attempt = attempt + 1;
                    sr.duration_ms = step_start.elapsed().as_millis() as u64;

                    if sr.status == StepStatus::Passed {
                        step_result = Some(sr.clone());
                        tracing::info!(
                            "[Self-Healing] StepPassed: index={}, name={}, attempt={}",
                            step_index,
                            step.name,
                            attempt + 1
                        );
                        on_event(&RunnerEvent::StepPassed {
                            step_index,
                            step_name: step.name.clone(),
                            attempt: attempt + 1,
                            total_attempts: total_executions,
                            duration_ms: sr.duration_ms,
                        });
                        last_attempt_error = None;
                        break;
                    } else {
                        // Step returned failed
                        step_result = Some(sr.clone());
                        last_attempt_error = sr.error.clone();
                        if let Some(err) = &last_attempt_error {
                            step_fatal = err.to_lowercase().starts_with("[fatal] ");
                        }
                        last_attempt_will_retry = attempt < *budget && !step_fatal;
                        tracing::info!(
                            "[Self-Healing] StepFailed: index={}, name={}, attempt={}, will_retry={}",
                            step_index,
                            step.name,
                            attempt + 1,
                            last_attempt_will_retry
                        );
                        on_event(&RunnerEvent::StepFailed {
                            step_index,
                            step_name: step.name.clone(),
                            attempt: attempt + 1,
                            total_attempts: total_executions,
                            error: sr.error.clone().unwrap_or_else(|| "Unknown error".into()),
                            will_retry: last_attempt_will_retry,
                        });
                        if attempt < *budget && !step_fatal {
                            result.total_attempts += 1;
                        }
                        if step_fatal {
                            tracing::warn!("FATAL error in step '{}': skipping retries", step.name);
                            break;
                        }
                    }
                }
                Err(e) => {
                    let failed = StepResult::failed(
                        &step.name,
                        attempt + 1,
                        e.clone(),
                        step_start.elapsed().as_millis() as u64,
                    );
                    step_result = Some(failed.clone());
                    last_attempt_error = Some(e.clone());
                    step_fatal = e.to_lowercase().starts_with("[fatal] ");
                    last_attempt_will_retry = attempt < *budget && !step_fatal;
                    on_event(&RunnerEvent::StepFailed {
                        step_index,
                        step_name: step.name.clone(),
                        attempt: attempt + 1,
                        total_attempts: total_executions,
                        error: e.clone(),
                        will_retry: last_attempt_will_retry,
                    });
                    result.total_attempts += 1;
                    if step_fatal {
                        tracing::warn!("FATAL error in step '{}': skipping retries", step.name);
                        break;
                    }
                }
            }

            // Check step timeout
            if let Some(timeout_s) = step.timeout_s {
                if step_start.elapsed() >= Duration::from_secs_f64(timeout_s) {
                    let timeout_err = format!("Step timeout after {timeout_s}s");
                    step_result = Some(StepResult::failed(
                        &step.name,
                        attempt + 1,
                        timeout_err.clone(),
                        step_start.elapsed().as_millis() as u64,
                    ));
                    last_attempt_error = Some(timeout_err.clone());
                    on_event(&RunnerEvent::StepFailed {
                        step_index,
                        step_name: step.name.clone(),
                        attempt: attempt + 1,
                        total_attempts: total_executions,
                        error: timeout_err,
                        will_retry: false,
                    });
                    break;
                }
            }
        }

        // Process result
        if let Some(sr) = step_result {
            if sr.status == StepStatus::Passed {
                result.passed_steps += 1;
                result.step_results.push(sr);
                step_index += 1;
                budget_map.remove(&step_index.wrapping_sub(1));
                continue;
            }

            // Failed after all retries — try recovery
            if plan.recovery_policy.enabled {
                let error = last_attempt_error
                    .clone()
                    .unwrap_or_else(|| sr.error.clone().unwrap_or_else(|| "Unknown error".into()));

                if let Some((hint, anchor_index)) = recovery::resolve_recovery(
                    &plan.recovery_policy,
                    step,
                    &error,
                    step_index,
                    plan.steps.len(),
                ) {
                    let action_label = match &hint.action {
                        RecoveryAction::SerialReset => "SerialReset".to_string(),
                        RecoveryAction::ProbeSoftReset => "ProbeSoftReset".to_string(),
                        RecoveryAction::ProbeHardReset => "ProbeHardReset".to_string(),
                        RecoveryAction::PowerCycle => "PowerCycle".to_string(),
                        RecoveryAction::Custom(s) => format!("Custom({})", s),
                        RecoveryAction::None => "None".to_string(),
                    };
                    let rewind_target = if anchor_index != step_index {
                        Some(anchor_index)
                    } else {
                        None
                    };
                    // Execute recovery action
                    let exec_outcome = recovery::execute_recovery(&hint.action);
                    let _outcome_msg = match &exec_outcome {
                        Ok(msg) => {
                            result.recovery_applied.push(format!("{}: {}", hint.reason, msg));
                            msg.clone()
                        }
                        Err(e) => {
                            result.recovery_applied.push(format!("{}: Recovery failed — {e}", hint.reason));
                            format!("Recovery failed — {e}")
                        }
                    };
                    tracing::info!(
                        "recovery: action={} reason={} step={}",
                        action_label, hint.reason, step.name
                    );
                    on_event(&RunnerEvent::RecoveryApplied {
                        step_index,
                        step_name: step.name.clone(),
                        action: action_label,
                        reason: hint.reason.clone(),
                        rewind_to: rewind_target,
                    });

                    // Rewind to anchor point
                    step_index = anchor_index.min(plan.steps.len().saturating_sub(1));
                    result.total_attempts += 1;
                    // Only reset budgets from the anchor point onwards.
                    // Previously this called `budget_map.clear()`, which reset
                    // ALL step budgets and could cause the runner to retry the
                    // same failing step until the global guard_limit was
                    // exhausted. By only resetting from the anchor, earlier
                    // successful steps keep their (already consumed) budgets
                    // and the runner converges faster.
                    budget_map.retain(|k, _| *k < anchor_index);
                    continue;
                }
            }

            // No recovery possible — Self-Healing failed
            result.step_results.push(sr);
            result.summary = format!(
                "Self-Healing FAILED at step '{}' (index {}) after {} total attempts.",
                step.name, step_index, total_executions
            );
            result.total_duration_ms = start_time.elapsed().as_millis() as u64;
            return result;
        } else {
            // No result (shouldn't happen)
            result.summary = format!("Unexpected: no result for step '{}'.", step.name);
            result.total_duration_ms = start_time.elapsed().as_millis() as u64;
            return result;
        }
    }

    // All steps passed
    result.passed = true;
    result.total_duration_ms = start_time.elapsed().as_millis() as u64;
    result.total_attempts = total_executions;
    result.summary = format!(
        "PASS: All {} steps verified ({}/{} passed, {} total attempts, {}ms). Recovery actions: {}",
        result.total_steps,
        result.passed_steps,
        result.total_steps,
        result.total_attempts,
        result.total_duration_ms,
        result.recovery_applied.len(),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan() -> Plan {
        Plan {
            name: "test".into(),
            board: "esp32".into(),
            test: "blink".into(),
            steps: vec![
                Step {
                    name: "preflight".into(),
                    category: StepCategory::Check,
                    description: "Check instruments".into(),
                    adapter: "check.instrument".into(),
                    params: serde_json::json!({}),
                    timeout_s: None,
                },
                Step {
                    name: "build".into(),
                    category: StepCategory::Build,
                    description: "Build firmware".into(),
                    adapter: "build.idf".into(),
                    params: serde_json::json!({}),
                    timeout_s: None,
                },
                Step {
                    name: "flash".into(),
                    category: StepCategory::Load,
                    description: "Flash firmware".into(),
                    adapter: "flash.idf_esptool".into(),
                    params: serde_json::json!({}),
                    timeout_s: None,
                },
                Step {
                    name: "verify".into(),
                    category: StepCategory::Check,
                    description: "Verify output".into(),
                    adapter: "verify.serial".into(),
                    params: serde_json::json!({}),
                    timeout_s: None,
                },
            ],
            recovery_policy: RecoveryPolicy::default(),
            timeout_s: None,
            guard_limit: Some(50),
        }
    }

    #[test]
    fn test_all_pass() {
        let plan = make_plan();
        let result = run_plan(&plan, &|step, _ctx| {
            Ok(StepResult::passed(&step.name, 1, 100))
        });
        assert!(result.passed);
        assert_eq!(result.passed_steps, 4);
    }

    #[test]
    fn test_fail_no_recovery() {
        let mut plan = make_plan();
        plan.recovery_policy.enabled = false;

        let result = run_plan(&plan, &|step, _ctx| {
            if step.name == "build" {
                Err("Compilation error".into())
            } else {
                Ok(StepResult::passed(&step.name, 1, 100))
            }
        });
        assert!(!result.passed);
    }

    #[test]
    fn test_retry_on_failure() {
        let plan = make_plan();
        use std::cell::RefCell;
        let flash_counter = RefCell::new(0);

        let result = run_plan(&plan, &|step, _ctx| {
            if step.name == "flash" {
                let mut c = flash_counter.borrow_mut();
                *c += 1;
                if *c <= 3 {
                    Err(format!("Flash failed attempt {}", *c))
                } else {
                    Ok(StepResult::passed(&step.name, 1, 100))
                }
            } else {
                Ok(StepResult::passed(&step.name, 1, 100))
            }
        });
        assert!(result.passed);
        assert!(result.recovery_applied.iter().any(|r| r.contains("flash")));
    }
}