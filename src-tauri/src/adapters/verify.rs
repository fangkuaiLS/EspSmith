//! Verify adapters — serial output, signal capture, mailbox, and GDB verification.

use super::*;
use std::io::Read;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Serial output verification adapter.
pub struct SerialVerifyAdapter;

impl Adapter for SerialVerifyAdapter {
    fn name(&self) -> &str { "verify.serial" }
    fn description(&self) -> &str { "Verify firmware via serial output pattern" }

    fn execute(&self, params: &serde_json::Value, _work_dir: &str) -> AdapterResult {
        let port = params.get("port").and_then(|v| v.as_str()).unwrap_or("COM3");
        let baudrate = params.get("baudrate").and_then(|v| v.as_u64()).unwrap_or(115200) as u32;
        let expected = params.get("expected_pattern").and_then(|v| v.as_str()).unwrap_or("");
        let monitor_ms = params.get("monitor_ms").and_then(|v| v.as_u64()).unwrap_or(5000);

        let start = Instant::now();

        // Open serial and read
        let mut serial = match serialport::new(port, baudrate)
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(s) => s,
            Err(e) => {
                return AdapterResult::fail(
                    format!("Failed to open serial port {port}: {e}"),
                    None,
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let deadline = Instant::now() + Duration::from_millis(monitor_ms);
        let mut output = Vec::new();
        let mut buf = [0u8; 512];

        while Instant::now() < deadline {
            match serial.read(&mut buf) {
                Ok(n) if n > 0 => {
                    output.extend_from_slice(&buf[..n]);
                    if output.len() > 4096 {
                        break;
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => {
                    return AdapterResult::fail(
                        format!("Serial read error: {e}"),
                        None,
                        start.elapsed().as_millis() as u64,
                    );
                }
            }
        }

        let text = String::from_utf8_lossy(&output).to_string();
        let duration = start.elapsed().as_millis() as u64;

        if expected.is_empty() {
            // No expected pattern — just return the output
            return AdapterResult::ok(
                Some(format!("Serial output ({}ms):\n{}", monitor_ms, text)),
                duration,
            );
        }

        // Check expected pattern
        if text.contains(expected) {
            AdapterResult::ok(
                Some(format!(
                    "Verified: found pattern '{}' in serial output ({})",
                    expected,
                    text.lines().find(|l| l.contains(expected)).unwrap_or("")
                )),
                duration,
            )
        } else {
            AdapterResult::fail(
                format!(
                    "Verification failed: pattern '{}' not found in serial output:\n{}",
                    expected,
                    text
                ),
                Some(text),
                duration,
            )
        }
    }
}

/// GDB verify adapter (runs a GDB batch command and checks output).
pub struct GdbVerifyAdapter;

impl Adapter for GdbVerifyAdapter {
    fn name(&self) -> &str { "verify.gdb" }
    fn description(&self) -> &str { "Verify state via GDB command" }

    fn execute(&self, params: &serde_json::Value, _work_dir: &str) -> AdapterResult {
        let command = params.get("command").and_then(|v| v.as_str()).unwrap_or("monitor reg");
        let target_chip = params.get("target_chip").and_then(|v| v.as_str());
        let gdb_binary = match crate::commands::gdb_session::find_gdb_binary(target_chip) {
            Ok(path) => path,
            Err(e) => {
                return AdapterResult::fail(
                    format!("GDB not found: {}", e),
                    Some(e),
                    0,
                );
            }
        };
        let start = Instant::now();

        let mut cmd = std::process::Command::new(&gdb_binary);
        cmd.args([
                "-batch", "-nx",
                "-ex", "target remote localhost:3333",
                "-ex", command,
            ]);
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        let output = cmd.output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let duration = start.elapsed().as_millis() as u64;

                if out.status.success() {
                    AdapterResult::ok(Some(stdout), duration)
                } else {
                    AdapterResult::fail(
                        format!("GDB command failed: {}", stderr),
                        Some(stderr),
                        duration,
                    )
                }
            }
            Err(e) => AdapterResult::fail(
                format!("GDB execution failed ({gdb_binary}): {e}"),
                None,
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}