//! confserver process manager — communicates with kconfserver via stdin/stdout JSON.
//!
//! Protocol reference: vscode-esp-idf-extension-master/src/espIdf/menuconfig/confServerProcess.ts
//!
//! Commands (stdin, one JSON per line):
//!   set:    {"version":2,"set":{"CONFIG_KEY":value}}
//!   save:   {"version":2,"save":"/path/to/sdkconfig"}
//!   load:   {"version":2,"load":"/path/to/sdkconfig"}
//!   reset:  {"version":3,"reset":["KEY"]}
//!
//! Responses (stdout, JSON blocks):
//!   {"version":2,"values":{...},"visible":{...},"ranges":{...},"defaults":{...}}
//!
//! Architecture:
//!   We call `python -m kconfserver` directly (bypassing `idf.py confserver`) to avoid
//!   the CMake target consistency check that `idf.py` performs. The build directory must
//!   already be configured (i.e. `build/config.env` exists) from a prior CMake run.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::idf;
use tracing::{info, warn};

pub struct ConfserverProcess {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    stderr_buf: std::sync::Arc<std::sync::Mutex<String>>,
}

impl ConfserverProcess {
    /// Start kconfserver directly (bypass idf.py to avoid CMake target check).
    /// Returns the process and the initial JSON response.
    pub fn start(project_path: &str, idf_path: &str) -> Result<(Self, serde_json::Value), String> {
        let build_dir = Path::new(project_path).join("build");
        let sdkconfig_path = Path::new(project_path).join("sdkconfig");

        // Sanitize sdkconfig: trim whitespace from all lines.
        if sdkconfig_path.exists() {
            if let Err(e) = sanitize_sdkconfig(&sdkconfig_path) {
                warn!("[confserver] Failed to sanitize sdkconfig: {}", e);
            }
        }

        let config_env = build_dir.join("config.env");

        // If build directory is already configured, call kconfserver directly.
        if config_env.exists() {
            return Self::start_kconfserver_direct(
                project_path, idf_path, &build_dir, &sdkconfig_path, &config_env,
            );
        }

        // Build directory not configured — run CMake first to generate config.env,
        // then call kconfserver.
        info!("[confserver] build/config.env not found, running idf.py reconfigure to generate it");
        Self::run_reconfigure(project_path, idf_path)?;

        if !config_env.exists() {
            return Err(format!(
                "Build directory not configured. config.env not found at {}. Please run Build first.",
                config_env.display()
            ));
        }

        Self::start_kconfserver_direct(
            project_path, idf_path, &build_dir, &sdkconfig_path, &config_env,
        )
    }

    /// Run `idf.py reconfigure` to generate build directory files (including config.env).
    fn run_reconfigure(project_path: &str, idf_path: &str) -> Result<(), String> {
        idf::run_idf_command(project_path, idf_path, &["reconfigure"])
            .map_err(|e| format!("Failed to run idf.py reconfigure: {}", e))?;
        info!("[confserver] idf.py reconfigure completed");
        Ok(())
    }

    /// Start kconfserver directly using the build directory's config.env.
    fn start_kconfserver_direct(
        project_path: &str, idf_path: &str,
        _build_dir: &Path, sdkconfig_path: &Path, config_env: &Path,
    ) -> Result<(Self, serde_json::Value), String> {
        // Normalize sdkconfig path to forward slashes (consistent with save command)
        let sdkconfig = sdkconfig_path.to_string_lossy().replace('\\', "/");
        let idf_kconfig = Path::new(idf_path).join("Kconfig");
        let config_env_str = config_env.to_string_lossy().replace('\\', "/");

        // Try EIM Python first, then fall back to export.bat
        if let Some(eim_setup) = idf::find_eim_setup(idf_path) {
            return Self::start_kconfserver_eim(
                project_path, idf_path, &eim_setup, &sdkconfig, &idf_kconfig, &config_env_str,
            );
        }

        // Fallback: export.bat
        #[cfg(windows)]
        {
            Self::start_kconfserver_export_bat(
                project_path, idf_path, &sdkconfig, &idf_kconfig, &config_env_str,
            )
        }
        #[cfg(not(windows))]
        { Err("kconfserver only supported on Windows via EIM or export.bat".to_string()) }
    }

    /// Start kconfserver using EIM Python environment.
    fn start_kconfserver_eim(
        project_path: &str, idf_path: &str,
        eim_setup: &idf::EimIdfInstalled,
        sdkconfig: &str, idf_kconfig: &Path, config_env: &str,
    ) -> Result<(Self, serde_json::Value), String> {
        let python = eim_setup.python.replace('/', "\\");
        if !Path::new(&python).exists() {
            return Err(format!("EIM Python not found: {}", python));
        }

        let system_path = std::env::var("PATH").unwrap_or_default();
        let py_scripts = Path::new(&python).parent()
            .map(|p| p.to_string_lossy().to_string()).unwrap_or_default();

        let eim_path_entries = idf::build_eim_path_entries(&eim_setup.idf_tools_path);
        let new_path = if eim_path_entries.is_empty() {
            format!("{};{}", py_scripts, system_path)
        } else {
            format!("{};{};{}", eim_path_entries.join(";"), py_scripts, system_path)
        };

        let idf_python_env_path = Path::new(&python)
            .parent().and_then(|p| p.parent())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let idf_tools = format!("{}\\tools", idf_path);

        // Step 1: Run prepare_kconfig_files.py to generate kconfigs.in / kconfigs_projbuild.in
        let prepare_py = Path::new(idf_path)
            .join("tools").join("kconfig_new").join("prepare_kconfig_files.py");
        if prepare_py.exists() {
            info!("[confserver] Running prepare_kconfig_files.py");
            let prepare_output = Command::new(&python)
                .arg(&prepare_py)
                .arg("--list-separator=semicolon")
                .arg("--env-file").arg(config_env)
                .env("IDF_PATH", idf_path)
                .env("IDF_TOOLS_PATH", &eim_setup.idf_tools_path)
                .env("IDF_PYTHON_ENV_PATH", &idf_python_env_path)
                .env("PATH", &new_path)
                .current_dir(project_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .creation_flags(0x08000000)
                .spawn()
                .and_then(|c| c.wait_with_output())
                .map_err(|e| format!("Failed to run prepare_kconfig_files.py: {}", e))?;
            if !prepare_output.status.success() {
                let stderr = String::from_utf8_lossy(&prepare_output.stderr);
                warn!("[confserver] prepare_kconfig_files.py stderr: {}", stderr.trim());
            }
        }

        // Step 2: Run kconfserver directly
        info!("[confserver] Starting kconfserver: python={}, kconfig={}, config={}, env-file={}",
            python, idf_kconfig.display(), sdkconfig, config_env);

        let mut cmd = Command::new(&python);
        cmd.args(["-m", "kconfserver"])
            .arg("--env-file").arg(config_env)
            .arg("--kconfig").arg(idf_kconfig)
            .arg("--config").arg(sdkconfig)
            .env("IDF_PATH", idf_path)
            .env("IDF_TOOLS_PATH", &eim_setup.idf_tools_path)
            .env("IDF_PYTHON_ENV_PATH", &idf_python_env_path)
            .env("ESP_IDF_VERSION", idf::get_idf_version_for_env(idf_path))
            .env("PATH", &new_path)
            .env("PYTHONPATH", format!("{};{}", &idf_tools, std::env::var("PYTHONPATH").unwrap_or_default()))
            .env("OPENOCD_SCRIPTS", format!("{}\\openocd-esp32", eim_setup.idf_tools_path))
            .current_dir(project_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)] { cmd.creation_flags(0x08000000); }

        Self::spawn_and_read_initial(cmd, "EIM")
    }

    /// Start kconfserver via export.bat (cmd) for non-EIM setups.
    #[cfg(windows)]
    fn start_kconfserver_export_bat(
        project_path: &str, idf_path: &str,
        sdkconfig: &str, idf_kconfig: &Path, config_env: &str,
    ) -> Result<(Self, serde_json::Value), String> {
        let export_bat = Path::new(idf_path).join("export.bat");
        if !export_bat.exists() {
            return Err(format!("export.bat not found at {}", export_bat.display()));
        }

        let prepare_py = Path::new(idf_path)
            .join("tools").join("kconfig_new").join("prepare_kconfig_files.py");

        let cmd_str = if prepare_py.exists() {
            format!(
                "call \"{}\" >nul 2>&1 && set ESP_IDF_VERSION={} && python \"{}\" --list-separator=semicolon --env-file \"{}\" && python -m kconfserver --env-file \"{}\" --kconfig \"{}\" --config \"{}\"",
                export_bat.display(),
                idf::get_idf_version_for_env(idf_path),
                prepare_py.display(), config_env,
                config_env, idf_kconfig.display(), sdkconfig,
            )
        } else {
            format!(
                "call \"{}\" >nul 2>&1 && set ESP_IDF_VERSION={} && python -m kconfserver --env-file \"{}\" --kconfig \"{}\" --config \"{}\"",
                export_bat.display(),
                idf::get_idf_version_for_env(idf_path),
                config_env, idf_kconfig.display(), sdkconfig,
            )
        };

        info!("[confserver] Starting kconfserver via export.bat");

        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &cmd_str])
            .current_dir(project_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)] { cmd.creation_flags(0x08000000); }

        Self::spawn_and_read_initial(cmd, "export.bat")
    }

    /// Spawn the process, set up stderr reading, and return the initial JSON response.
    fn spawn_and_read_initial(
        mut cmd: Command, source: &str,
    ) -> Result<(Self, serde_json::Value), String> {
        let mut child = cmd.spawn()
            .map_err(|e| format!("Failed to start confserver via {}: {}", source, e))?;

        let stdin = child.stdin.take().ok_or("confserver stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("confserver stdout unavailable")?;

        let stderr_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let stderr_buf_clone = stderr_buf.clone();
        if let Some(stderr) = child.stderr.take() {
            let stderr_reader = BufReader::new(stderr);
            std::thread::spawn(move || {
                for line in stderr_reader.lines() {
                    if let Ok(text) = line {
                        let trimmed = text.trim();
                        if trimmed.is_empty() { continue; }
                        if trimmed.starts_with("Server running")
                            || trimmed.starts_with("Saving config")
                            || trimmed.starts_with("Loading config")
                            || trimmed.contains("not visible so were not updated")
                            || trimmed.starts_with("WARNING:") {
                            info!("[confserver stderr] {}", trimmed);
                        } else {
                            warn!("[confserver stderr] {}", trimmed);
                        }
                        if let Ok(mut buf) = stderr_buf_clone.lock() {
                            buf.push_str(trimmed);
                            buf.push('\n');
                        }
                    }
                }
            });
        }

        let reader = BufReader::new(stdout);
        let mut process = ConfserverProcess { child, stdin, reader, stderr_buf };

        let initial = process.read_response()
            .map_err(|e| format!("confserver: no initial response: {}", e))?;

        Ok((process, initial))
    }

    /// Read one JSON response from stdout. Blocks until a complete JSON block is received.
    ///
    /// The confserver may print debug messages (e.g. "Set CONFIG_FOO") to stdout
    /// alongside JSON responses. We skip non-JSON lines and only parse JSON objects.
    fn read_response(&mut self) -> Result<serde_json::Value, String> {
        let mut buf = String::new();
        let mut line_count = 0u32;
        loop {
            let mut line = String::new();
            info!("[confserver::read_response] Waiting for line {} from stdout...", line_count + 1);
            let n = self.reader.read_line(&mut line)
                .map_err(|e| format!("confserver read error: {}", e))?;
            line_count += 1;
            info!("[confserver::read_response] Read {} bytes, line {}: {:?}", n, line_count, line.trim_end());
            if n == 0 {
                // stdout closed — always include stderr for diagnostics
                let exit_status = self.child.try_wait().ok().flatten();
                let exit_info = match exit_status {
                    Some(s) => format!("exited with {}", s),
                    None => "still running".to_string(),
                };
                let stderr = self.stderr_buf.lock().map(|b| b.clone()).unwrap_or_default();
                let partial = if buf.trim().is_empty() {
                    "(no output)".to_string()
                } else {
                    buf[..buf.len().min(300)].to_string()
                };
                return Err(format!(
                    "confserver stdout closed ({}) | partial stdout: {} | stderr: {}",
                    exit_info, partial.trim(), stderr.trim()
                ));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse this line as JSON directly.
            // The confserver sends each JSON response on a single line.
            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(val) => {
                    info!("[confserver] Received response, version={}", val.get("version").and_then(|v| v.as_i64()).unwrap_or(0));
                    return Ok(val);
                }
                Err(_) => {
                    // Non-JSON line (e.g. "Set CONFIG_FOO" debug message from confserver).
                    // Accumulate it for diagnostics, then skip it.
                    warn!("[confserver::read_response] Skipping non-JSON stdout line: {}", trimmed);
                    buf.push_str(&line);
                    continue;
                }
            }
        }
    }

    /// Send a JSON command via stdin and wait for the response.
    pub fn send_command(&mut self, cmd: &str) -> Result<serde_json::Value, String> {
        info!("[confserver] Sending: {}", cmd);
        self.stdin.write_all(cmd.as_bytes())
            .map_err(|e| format!("confserver write error: {}", e))?;
        self.stdin.write_all(b"\n")
            .map_err(|e| format!("confserver write error: {}", e))?;
        self.stdin.flush()
            .map_err(|e| format!("confserver flush error: {}", e))?;

        self.read_response()
    }

    /// Check if a confserver response contains errors.
    fn check_response_errors(response: &serde_json::Value) -> Option<String> {
        if let Some(errors) = response.get("error") {
            if let Some(arr) = errors.as_array() {
                if !arr.is_empty() {
                    let msgs: Vec<String> = arr.iter()
                        .filter_map(|e| e.as_str().map(|s| s.to_string()))
                        .collect();
                    return Some(msgs.join("; "));
                }
            } else if let Some(s) = errors.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
        None
    }

    /// Set a configuration value.
    pub fn set_value(&mut self, key: &str, value: &serde_json::Value) -> Result<serde_json::Value, String> {
        let cmd = format!(r#"{{"version":2,"set":{{"{}":{}}}}}"#, key, value);
        let response = self.send_command(&cmd)?;
        if let Some(err) = Self::check_response_errors(&response) {
            return Err(format!("confserver set_value '{}' failed: {}", key, err));
        }
        Ok(response)
    }

    /// Save current configuration to sdkconfig file.
    pub fn save(&mut self, sdkconfig_path: &str) -> Result<(), String> {
        let cmd = format!(r#"{{"version":2,"save":"{}"}}"#, sdkconfig_path.replace('\\', "/"));
        let response = self.send_command(&cmd)?;
        if let Some(err) = Self::check_response_errors(&response) {
            return Err(format!("confserver save failed: {}", err));
        }
        info!("[confserver] Saved sdkconfig to {}", sdkconfig_path);
        Ok(())
    }

    /// Load configuration from sdkconfig file.
    pub fn load(&mut self, sdkconfig_path: &str) -> Result<serde_json::Value, String> {
        let cmd = format!(r#"{{"version":2,"load":"{}"}}"#, sdkconfig_path.replace('\\', "/"));
        self.send_command(&cmd)
    }

    /// Kill the confserver process.
    pub fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Clean sdkconfig file: normalize line endings to \n and trim trailing whitespace
/// from every line. This avoids CMake target mismatch errors caused by stray
/// spaces/newlines in values like CONFIG_IDF_TARGET.
fn sanitize_sdkconfig(path: &Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read sdkconfig: {}", e))?;

    // Normalize \r\n → \n and trim trailing whitespace from each line
    let cleaned: String = content
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    info!("[confserver] Sanitized sdkconfig (normalized line endings & trimmed whitespace)");
    std::fs::write(path, &cleaned)
        .map_err(|e| format!("Cannot write sdkconfig: {}", e))
}
