//! GDB debug adapter — interactive and batch GDB operations.

use super::*;
use std::process::Command;
use std::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Batch GDB command adapter.
pub struct GdbDebugAdapter {
    gdb_binary: String,
}

impl GdbDebugAdapter {
    pub fn new(gdb_binary: impl Into<String>) -> Self {
        Self { gdb_binary: gdb_binary.into() }
    }

    pub fn xtensa() -> Self {
        Self::new("xtensa-esp32-elf-gdb")
    }

    #[allow(dead_code)] // ARM调试适配器预留
    pub fn arm() -> Self {
        Self::new("arm-none-eabi-gdb")
    }

    #[allow(dead_code)] // RISC-V调试适配器预留
    pub fn riscv() -> Self {
        Self::new("riscv-none-elf-gdb")
    }
}

impl Adapter for GdbDebugAdapter {
    fn name(&self) -> &str { "gdb.debug" }
    fn description(&self) -> &str { "Run GDB batch command" }

    fn execute(&self, params: &serde_json::Value, _work_dir: &str) -> AdapterResult {
        let command = params.get("command").and_then(|v| v.as_str()).unwrap_or("info registers");
        let target = params.get("target").and_then(|v| v.as_str()).unwrap_or(super::GDB_ADDR);

        let start = Instant::now();
        let mut cmd = Command::new(&self.gdb_binary);
        cmd.args([
                "-batch", "-nx",
                "-ex", &format!("target remote {}", target),
                "-ex", command,
            ]);
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        match cmd.output()
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let duration = start.elapsed().as_millis() as u64;

                if out.status.success() && !stderr.contains("error") {
                    AdapterResult::ok(Some(stdout), duration)
                } else {
                    AdapterResult::fail(
                        format!("GDB error: {}", stderr),
                        Some(format!("stdout:\n{}\nstderr:\n{}", stdout, stderr)),
                        duration,
                    )
                }
            }
            Err(e) => AdapterResult::fail(
                format!("Failed to start GDB ({}): {}", self.gdb_binary, e),
                None,
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}