//! Build adapter — wraps ESP-IDF build and other build systems.

use super::*;
use std::process::Command;
use std::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// IDF build adapter (wraps idf.py build).
pub struct IdfBuildAdapter;

impl Adapter for IdfBuildAdapter {
    fn name(&self) -> &str { "build.idf" }
    fn description(&self) -> &str { "ESP-IDF 6.x idf.py build" }

    fn execute(&self, params: &serde_json::Value, work_dir: &str) -> AdapterResult {
        let idf_path = params.get("idf_path")
            .and_then(|v| v.as_str()).unwrap_or("");
        let _extra_args: Vec<&str> = params.get("extra_args")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let start = Instant::now();
        match crate::idf::build_sync(work_dir, idf_path) {
            Ok(output) => {
                AdapterResult::ok(
                    Some(output),
                    start.elapsed().as_millis() as u64,
                )
            }
            Err(output) => {
                let real_errors: Vec<_> = crate::idf::parse_compile_errors(&output)
                    .into_iter()
                    .filter(|e| {
                        let msg = e.message.to_lowercase();
                        let is_warning = e.error_type == "warning"
                            || msg.contains("warning:")
                            || msg.starts_with("[0;33m");
                        let is_log_issue = msg.contains("permission denied")
                            || msg.contains("filenotfounderror")
                            || msg.contains("no such file or directory");
                        !is_warning && !is_log_issue
                    })
                    .collect();

                if real_errors.is_empty() {
                    let bin_name = std::path::Path::new(work_dir)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let bin_path = std::path::Path::new(work_dir)
                        .join("build")
                        .join(format!("{}.bin", bin_name));
                    if bin_path.exists() {
                        AdapterResult::ok(
                            Some(format!("Build completed (with warnings):\n{}", output)),
                            start.elapsed().as_millis() as u64,
                        )
                    } else {
                        AdapterResult::fail(
                            format!("Build failed (no output binary):\n{}", output),
                            Some(output),
                            start.elapsed().as_millis() as u64,
                        )
                    }
                } else {
                    let err_msg = format!(
                        "Build failed with {} errors:\n{}",
                        real_errors.len(),
                        real_errors.iter().map(|e| {
                            format!("{}:{}:{} - {}", e.file, e.line, e.column, e.message)
                        }).collect::<Vec<_>>().join("\n")
                    );
                    AdapterResult::fail(err_msg, Some(output), start.elapsed().as_millis() as u64)
                }
            }
        }
    }
}

/// Generic build adapter that runs a custom command.
#[allow(dead_code)] // 通用构建适配器预留
pub struct GenericBuildAdapter {
    cmd: String,
    args: Vec<String>,
}

impl GenericBuildAdapter {
    #[allow(dead_code)] // 通用构建适配器预留
    pub fn new(cmd: impl Into<String>, args: Vec<String>) -> Self {
        Self { cmd: cmd.into(), args }
    }
}

impl Adapter for GenericBuildAdapter {
    fn name(&self) -> &str { "build.generic" }
    fn description(&self) -> &str { "Run a custom build command" }

    fn execute(&self, _params: &serde_json::Value, work_dir: &str) -> AdapterResult {
        let start = Instant::now();
        let mut cmd = Command::new(&self.cmd);
        cmd.args(&self.args).current_dir(work_dir);
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        match cmd.output()
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let duration = start.elapsed().as_millis() as u64;
                if out.status.success() {
                    AdapterResult::ok(Some(stdout), duration)
                } else {
                    AdapterResult::fail(
                        format!("Build failed: {}", stderr.lines().last().unwrap_or("unknown error")),
                        Some(stderr),
                        duration,
                    )
                }
            }
            Err(e) => AdapterResult::fail(
                format!("Failed to run build command: {e}"),
                None,
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}