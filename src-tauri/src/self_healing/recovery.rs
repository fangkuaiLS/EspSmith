//! Recovery policy resolution — mirrors AEL's recovery_policy.py / failure_recovery.py.
//!
//! Decides what recovery action to take and at what anchor point to retry from,
//! based on the failure mode and the configured Self-Healing recovery policy.

use super::types::*;

/// Resolve a recovery hint from a failure context.
///
/// Returns (RecoveryHint, new_anchor_index) or None if recovery is not possible.
pub fn resolve_recovery(
    policy: &RecoveryPolicy,
    failed_step: &Step,
    error_message: &str,
    step_index: usize,
    total_steps: usize,
) -> Option<(RecoveryHint, usize)> {
    if !policy.enabled {
        return None;
    }

    let lower = error_message.to_lowercase();
    if lower.starts_with("[fatal] ") {
        tracing::warn!(
            "FATAL error in step '{}': '{}'. Skipping recovery (fatal errors are not recoverable).",
            failed_step.name, error_message
        );
        return None;
    }
    if lower.contains("gdb") && (lower.contains("timeout") || lower.contains("timed out")
        || lower.contains("connection refused") || lower.contains("cannot connect")
        || lower.contains("not available") || lower.contains("not reachable")) {
        return None;
    }

    let (anchor, anchor_index) = classify_failure(failed_step, error_message, step_index, total_steps);

    let action = pick_recovery_action(policy, failed_step, error_message);

    Some((
        RecoveryHint {
            action: action.clone(),
            anchor: anchor.clone(),
            reason: format!(
                "Step '{}' (category={:?}) failed: {}. Rewinding to anchor={:?}.",
                failed_step.name, failed_step.category, error_message, anchor
            ),
        },
        anchor_index,
    ))
}

/// Classify failure to determine anchor point.
fn classify_failure(
    step: &Step,
    error: &str,
    step_index: usize,
    _total_steps: usize,
) -> (AnchorPoint, usize) {
    let lower = error.to_lowercase();

    if lower.contains("gdb") && (lower.contains("timeout") || lower.contains("timed out")
        || lower.contains("connection refused") || lower.contains("cannot connect")) {
        return (AnchorPoint::Check, step_index);
    }

    if lower.contains("flash") || lower.contains("openocd") || lower.contains("gdb")
        || lower.contains("probe") || lower.contains("jtag") || lower.contains("swd")
    {
        return (AnchorPoint::Load, step_index);
    }

    // Build errors: cannot recover, just rewind to build
    if lower.contains("compile") || lower.contains("build") || lower.contains("cmake")
        || lower.contains("undefined reference") || lower.contains("syntax error")
    {
        return (AnchorPoint::Build, step_index);
    }

    // Serial/verify errors: rewind to load (re-flash)
    if lower.contains("serial") || lower.contains("uart") || lower.contains("timeout")
        || lower.contains("no output") || lower.contains("verify")
    {
        return (AnchorPoint::Load, step_index);
    }

    // Default: rewind to current step
    match step.category {
        StepCategory::Build => (AnchorPoint::Build, step_index),
        StepCategory::Load => (AnchorPoint::Load, step_index),
        StepCategory::Check => (AnchorPoint::Check, step_index),
    }
}

/// Pick the most appropriate recovery action from the allowed list.
fn pick_recovery_action<'a>(
    policy: &'a RecoveryPolicy,
    _step: &Step,
    error: &str,
) -> &'a RecoveryAction {
    let lower = error.to_lowercase();

    // OpenOCD specific issues → probe hard reset
    if lower.contains("openocd") {
        for action in &policy.allowed_actions {
            if matches!(action, RecoveryAction::ProbeHardReset) {
                return action;
            }
        }
    }

    // Probe/mcu locked up → probe reset (soft preferred, hard fallback)
    if lower.contains("flash")
        || lower.contains("program")
        || lower.contains("verify")
        || lower.contains("gdb")
        || lower.contains("probe")
        || lower.contains("jtag")
        || lower.contains("swd")
        || lower.contains("telnet")
        || lower.contains("connection refused")
    {
        for action in &policy.allowed_actions {
            if matches!(action, RecoveryAction::ProbeSoftReset | RecoveryAction::ProbeHardReset) {
                return action;
            }
        }
    }

    // Serial timeout → serial reset
    if lower.contains("serial") || lower.contains("uart") || lower.contains("timeout") {
        for action in &policy.allowed_actions {
            if matches!(action, RecoveryAction::SerialReset) {
                return action;
            }
        }
    }

    // Power/connection issues → power cycle
    if lower.contains("power") || lower.contains("connection") || lower.contains("usb") {
        for action in &policy.allowed_actions {
            if matches!(action, RecoveryAction::PowerCycle) {
                return action;
            }
        }
    }

    // Build-related errors (compile/cmake/linker) → fall through to default

    // Default: return the first allowed action, or None
    policy.allowed_actions.first().unwrap_or(&RecoveryAction::None)
}

/// GDB 会话恢复上下文（闭包保持期间使用的 ELF 路径和芯片信息）。
static GDB_RECOVERY_CTX: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// 注册 GDB 恢复上下文，供 recovery 后自动重连使用。
pub fn set_gdb_recovery_context(elf_path: String, target_chip: String) {
    if let Ok(mut guard) = GDB_RECOVERY_CTX.lock() {
        *guard = Some((elf_path, target_chip));
    }
}

/// 清除 GDB 恢复上下文。
pub fn clear_gdb_recovery_context() {
    if let Ok(mut guard) = GDB_RECOVERY_CTX.lock() {
        *guard = None;
    }
}

/// 恢复后自动重连 GDB 会话。
fn reconnect_gdb_after_recovery() -> Result<String, String> {
    let ctx = GDB_RECOVERY_CTX.lock().map_err(|e| e.to_string())?;
    match ctx.as_ref() {
        Some((elf, chip)) => {
            crate::commands::gdb_session::disconnect_session_sync();
            std::thread::sleep(std::time::Duration::from_millis(500));
            crate::commands::gdb_session::connect_session_sync(elf, crate::adapters::GDB_ADDR, chip)?;
            Ok(format!("GDB session reconnected for {chip}"))
        }
        None => Ok("No GDB recovery context; skipping GDB reconnect.".into()),
    }
}

/// Execute a recovery action with real hardware interaction.
/// After probe resets, automatically reconnects the persistent GDB session.
pub fn execute_recovery(action: &RecoveryAction) -> Result<String, String> {
    let msg = match action {
        RecoveryAction::None => "No recovery action taken.".into(),
        RecoveryAction::SerialReset => {
            crate::commands::serial::serial_reset_via_dtr_rts()?
        }
        RecoveryAction::ProbeSoftReset => {
            crate::commands::serial::probe_soft_reset()?
        }
        RecoveryAction::ProbeHardReset => {
            probe_hard_reset_via_openocd()?
        }
        RecoveryAction::PowerCycle => {
            tracing::warn!("Power cycle requested but requires manual intervention");
            return Err("Power cycle requires manual intervention: please unplug and reconnect the device USB cable, then retry.".into());
        }
        RecoveryAction::Custom(desc) => {
            format!("Custom recovery: {}", desc)
        }
    };

    // 探针复位后自动重连 GDB 会话
    if matches!(action, RecoveryAction::ProbeSoftReset | RecoveryAction::ProbeHardReset) {
        match reconnect_gdb_after_recovery() {
            Ok(gdb_msg) => return Ok(format!("{}. {}", msg, gdb_msg)),
            Err(e) => return Ok(format!("{}. GDB reconnect failed: {}", msg, e)),
        }
    }

    Ok(msg)
}

/// 通过 OpenOCD telnet 接口执行硬复位。
fn probe_hard_reset_via_openocd() -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect_timeout(
        &crate::adapters::openocd_addr(),
        std::time::Duration::from_secs(2),
    ).map_err(|e| format!("Cannot connect to OpenOCD telnet (port 4444): {}. Is OpenOCD running?", e))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;

    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);

    stream
        .write_all(b"reset\n")
        .map_err(|e| format!("Failed to send reset command: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(500));

    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("Read error after reset: {}", e))?;

    Ok(format!(
        "Probe hard reset executed. Response: {}",
        String::from_utf8_lossy(&buf[..n]).trim()
    ))
}