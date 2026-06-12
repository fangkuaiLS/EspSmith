//! Self-Healing stage builders — construct standard embedded debug stages.
//!
//! Each function returns a Step that fits the AEL-inspired Self-Healing engine:
//!   plan → preflight → build → flash → verify → report

use super::types::*;

/// Create a preflight check step.
pub fn preflight(board: &str, port: &str) -> Step {
    Step {
        name: "preflight".into(),
        category: StepCategory::Check,
        description: format!("Preflight checks for board {board} on {port}"),
        adapter: "check.preflight".into(),
        params: serde_json::json!({"board": board, "port": port}),
        timeout_s: Some(30.0),
    }
}

/// Create a build step.
pub fn build(builder: &str, extra_args: &[&str]) -> Step {
    Step {
        name: format!("build.{builder}"),
        category: StepCategory::Build,
        description: format!("Build firmware using {builder}"),
        adapter: format!("build.{builder}"),
        params: serde_json::json!({"extra_args": extra_args}),
        timeout_s: Some(120.0),
    }
}

pub fn flash(chip: &str, port: &str) -> Step {
    Step {
        name: "flash.idf_esptool".into(),
        category: StepCategory::Load,
        description: format!("Flash firmware to {chip} via esptool (serial)"),
        adapter: "flash.idf_esptool".into(),
        params: serde_json::json!({
            "chip": chip,
            "port": port,
        }),
        timeout_s: Some(90.0),
    }
}

pub fn openocd_flash(chip: &str, port: &str) -> Step {
    Step {
        name: "flash.openocd".into(),
        category: StepCategory::Load,
        description: format!("Flash firmware to {chip} via OpenOCD JTAG (recommended)"),
        adapter: "flash.openocd".into(),
        params: serde_json::json!({
            "chip": chip,
            "port": port,
        }),
        timeout_s: Some(120.0),
    }
}

/// Create a serial verify step.
pub fn serial_verify(port: &str, baudrate: u32, expected_pattern: &str) -> Step {
    Step {
        name: "verify.serial".into(),
        category: StepCategory::Check,
        description: format!("Verify serial output on {port}"),
        adapter: "verify.serial".into(),
        params: serde_json::json!({
            "port": port,
            "baudrate": baudrate,
            "expected_pattern": expected_pattern,
            "monitor_ms": 5000
        }),
        timeout_s: Some(30.0),
    }
}

/// Create a GDB verify step.
#[allow(dead_code)] // Self-Healing验证阶段预留
pub fn gdb_verify(command: &str) -> Step {
    Step {
        name: "verify.gdb".into(),
        category: StepCategory::Check,
        description: format!("Verify state via GDB command: {command}"),
        adapter: "verify.gdb".into(),
        params: serde_json::json!({"command": command}),
        timeout_s: Some(15.0),
    }
}

/// Create a GDB session verify step — uses persistent GDB session.
pub fn gdb_session_verify(expected_pc_mask: &str, expected_regs: Option<serde_json::Value>, min_stack_depth: u64, target_chip: &str) -> Step {
    let mut params = serde_json::json!({
        "expected_pc_mask": expected_pc_mask,
        "min_stack_depth": min_stack_depth,
        "target_chip": target_chip,
    });
    if let Some(regs) = expected_regs {
        if let Some(obj) = params.as_object_mut() {
            obj.insert("expected_regs".into(), regs);
        }
    }
    Step {
        name: "verify.gdb_session".into(),
        category: StepCategory::Check,
        description: "Verify device state via persistent GDB session".into(),
        adapter: "verify.gdb_session".into(),
        params,
        timeout_s: Some(15.0),
    }
}

/// Create a signal capture verify step.
#[allow(dead_code)] // 信号采集阶段预留
pub fn signal_capture(pin: &str, expected: &str) -> Step {
    Step {
        name: format!("verify.signal.{pin}"),
        category: StepCategory::Check,
        description: format!("Capture signal on pin {pin}"),
        adapter: "verify.signal".into(),
        params: serde_json::json!({"pin": pin, "expected": expected}),
        timeout_s: Some(20.0),
    }
}