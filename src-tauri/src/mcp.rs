use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::adapters;
use crate::experience;
use crate::connection;
use crate::self_healing::{self, runner, types::*};
use crate::adapters::flash::find_elf_in_build_dir;

#[derive(Debug, Serialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

pub struct MCPServer {
    project_root: PathBuf,
    idf_path: Option<String>,
    /// Optional sink called by long-running tools (e.g. `closed_loop`) at
    /// every meaningful Self-Healing transition. The Tauri AI assistant wires
    /// this up to a Tauri event emitter so the frontend can show
    /// OperationTimeline.
    #[allow(clippy::type_complexity)]
    progress_sink: Option<Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync>>,
}

impl MCPServer {
    pub fn from_env() -> Result<Self, String> {
        let project_root = std::env::var("ESPSMITH_PROJECT")
            .map_err(|_| "ESPSMITH_PROJECT is not set".to_string())?;
        let idf_path = std::env::var("ESPSMITH_IDF_PATH").ok();
        Self::new(project_root, idf_path)
    }

    pub fn new(project_root: String, idf_path: Option<String>) -> Result<Self, String> {
        let project_root = PathBuf::from(project_root)
            .canonicalize()
            .map_err(|e| format!("Invalid project path: {e}"))?;

        let exp_dir = dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("espsmith")
            .join("experience");
        experience::init(exp_dir);

        Ok(Self { project_root, idf_path, progress_sink: None })
    }

    /// Attach (or replace) a runner-event sink. Used by the AI assistant
    /// thread so `closed_loop` can stream its retries / recovery actions
    /// to the frontend in real time.
    pub fn with_progress_sink(
        mut self,
        sink: Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync>,
    ) -> Self {
        self.progress_sink = Some(sink);
        self
    }

    pub fn list_tools(&self) -> Vec<Tool> {
        vec![
            tool("list_directory", "List files under a project-relative directory.", json!({
                "type": "object",
                "properties": { "path": { "type": "string", "default": "." } }
            })),
            tool("read_file", "Read a UTF-8 text file from the ESP-IDF project.", json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            })),
            tool("write_file", "Write a UTF-8 text file inside the ESP-IDF project. Creates parent folders. Do NOT write to hardware_pins.h (auto-generated).", json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            })),
            tool("build_project", "Run ESP-IDF 6.0 build and return output plus parsed compiler diagnostics. IMPORTANT: do NOT pass target unless changing chip — set-target triggers full reconfiguration and is very slow.", json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Target chip to set before building (e.g. esp32, esp32s3). ONLY use this when changing the chip target — it runs idf.py set-target first which triggers a full reconfiguration. Omit for normal builds." }
                }
            })),
            tool("flash_project", "Flash the built firmware to a serial port.", json!({
                "type": "object",
                "properties": { "port": { "type": "string" } },
                "required": ["port"]
            })),
            tool("build_flash_monitor", "[UART ONLY] Build, flash, then sample serial output. For JTAG use closed_loop instead.", json!({
                "type": "object",
                "properties": {
                    "port": { "type": "string" },
                    "baudrate": { "type": "integer", "default": 115200 },
                    "monitor_ms": { "type": "integer", "default": 5000 }
                },
                "required": ["port"]
            })),
            tool("list_serial_ports", "List available serial ports with JTAG capability detection.", json!({
                "type": "object",
                "properties": {}
            })),
            tool("detect_connection", "Detect whether the device is connected via USB-JTAG or UART. Returns mode, capabilities, and recommendations. Call this before closed_loop to determine the best flash & verify path.", json!({
                "type": "object",
                "properties": {
                    "port": { "type": "string", "description": "Serial port to check (e.g. COM3). If omitted, scans all ports." }
                }
            })),
            tool("get_connection_mode", "Get the cached connection mode (JTAG or UART) from the last detection.", json!({
                "type": "object",
                "properties": {}
            })),
            tool("jtag_runtime_check", "JTAG deep runtime check: start OpenOCD, connect GDB, set breakpoints, run the program, capture serial output + GDB state. Use ONLY when you need to set breakpoints or watch variables. For general verification, use closed_loop instead. If this fails, fall back to closed_loop — do NOT manually invoke GDB.", json!({
                "type": "object",
                "properties": {
                    "port": { "type": "string", "description": "Serial port (e.g. COM3)" },
                    "chip": { "type": "string", "description": "Target chip (e.g. esp32s3, esp32c3)" },
                    "elf_path": { "type": "string", "description": "Path to ELF file (e.g. build/app.elf)" },
                    "baudrate": { "type": "integer", "default": 115200 },
                    "monitor_ms": { "type": "integer", "default": 5000 },
                    "expected_pattern": { "type": "string", "description": "Expected text in serial output" },
                    "breakpoints": { "type": "array", "items": { "type": "string" }, "description": "Breakpoints as 'function' or 'file:line' (e.g. ['app_main', 'main/hello_world_main.c:45'])" },
                    "watch_variables": { "type": "array", "items": { "type": "string" }, "description": "Variable names to read at breakpoints (e.g. ['counter', 'state'])" }
                },
                "required": ["port", "chip"]
            })),
            tool("read_serial", "Open a serial port briefly and return observed output.", json!({
                "type": "object",
                "properties": {
                    "port": { "type": "string" },
                    "baudrate": { "type": "integer", "default": 115200 },
                    "duration_ms": { "type": "integer", "default": 3000 }
                },
                "required": ["port"]
            })),
            tool("get_hardware_config", "Read .espsmith/hardware_config.json.", json!({
                "type": "object",
                "properties": {}
            })),
            tool("export_hardware_header", "Generate hardware_config.h content from the hardware configuration.", json!({
                "type": "object",
                "properties": {}
            })),
            tool("run_gdb_command", "Run one GDB batch command (auto-selects correct GDB for chip, multi-arch support).", json!({
                "type": "object",
                "properties": { "command": { "type": "string" } },
                "required": ["command"]
            })),
            tool("openocd_start", "Start OpenOCD for a specific chip (auto-detects interface and target config).", json!({
                "type": "object",
                "properties": { "chip": { "type": "string", "description": "Target chip (e.g. esp32s3, esp32c3)" } }
            })),
            tool("openocd_stop", "Stop the running OpenOCD process.", json!({
                "type": "object", "properties": {}
            })),
            tool("openocd_is_running", "Check if OpenOCD is currently running.", json!({
                "type": "object", "properties": {}
            })),
            tool("debug_start", "Start persistent GDB debug session (connect to target via OpenOCD).", json!({
                "type": "object",
                "properties": {
                    "elf_path": { "type": "string", "description": "Path to ELF file (e.g. build/app.elf)" },
                    "target": { "type": "string", "default": "localhost:3333" },
                    "target_chip": { "type": "string", "description": "Target chip for GDB binary selection" }
                },
                "required": ["elf_path"]
            })),
            tool("debug_stop", "Stop persistent GDB debug session.", json!({
                "type": "object", "properties": {}
            })),
            tool("debug_set_breakpoint", "Set a breakpoint at file:line via persistent GDB session.", json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "line": { "type": "integer" }
                },
                "required": ["file", "line"]
            })),
            tool("debug_continue", "Continue execution via persistent GDB session.", json!({
                "type": "object", "properties": {}
            })),
            tool("debug_step_over", "Step over current line via persistent GDB session.", json!({
                "type": "object", "properties": {}
            })),
            tool("debug_get_state", "Get current debug state (PC, stack, registers, breakpoints).", json!({
                "type": "object", "properties": {}
            })),
            tool("debug_read_variable", "Read a variable value via persistent GDB session.", json!({
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            })),
            tool("debug_get_registers", "Read all CPU registers via persistent GDB session.", json!({
                "type": "object", "properties": {}
            })),
            tool("debug_get_backtrace", "Get call stack (backtrace) via persistent GDB session.", json!({
                "type": "object", "properties": {}
            })),
            tool("project_context", "Return project, ESP-IDF and closed-loop guidance context.", json!({
                "type": "object",
                "properties": {}
            })),
            tool("closed_loop", "AEL-style one-click closed loop: preflight → build → flash → verify, with automatic retry & recovery. AUTO-DETECTS JTAG vs UART and selects the best path: JTAG (OpenOCD flash + GDB verify) when USB-JTAG detected, UART (esptool flash + serial verify) otherwise. JTAG mode is recommended for better debugging.", json!({
                "type": "object",
                "properties": {
                    "port": { "type": "string", "description": "Serial port (e.g. COM3)" },
                    "board": { "type": "string", "description": "Target board/chip (e.g. esp32, esp32s3, esp32c3). Auto-detected from project config if omitted." },
                    "baudrate": { "type": "integer", "default": 115200 },
                    "monitor_ms": { "type": "integer", "default": 5000 },
                    "expected_pattern": { "type": "string", "description": "Expected text in serial output (e.g. 'Hello World')" },
                    "force_jtag": { "type": "boolean", "description": "Force JTAG mode even if auto-detection says UART (for manual override)" },
                    "force_uart": { "type": "boolean", "description": "Force UART mode even if USB-JTAG is detected" },
                    "elf_path": { "type": "string", "description": "Path to ELF file for GDB verification (e.g. build/app.elf). Auto-detected in JTAG mode." },
                    "expected_pc_mask": { "type": "string", "default": "0x40000000", "description": "Minimum valid PC address mask for GDB verification" }
                },
                "required": ["port"]
            })),
            tool("query_experience", "Query accumulated engineering experience for a board/chip. Returns run statistics, known skills, likely pitfalls, and observation focus areas from previous runs.", json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Board/chip to query (e.g. esp32, esp32s3)" },
                    "test": { "type": "string", "default": "verify", "description": "Test scenario name" }
                },
                "required": ["board"]
            })),
            tool("record_experience", "Record an engineering skill or lesson learned for future reference. Use this when you discover a fix, workaround, or important pattern.", json!({
                "type": "object",
                "properties": {
                    "trigger": { "type": "string", "description": "When this skill applies (symptom or condition)" },
                    "fix": { "type": "string", "description": "The exact resolution or action" },
                    "lesson": { "type": "string", "description": "Reusable lesson or rule" },
                    "scope": { "type": "string", "default": "all", "description": "Applicability scope (e.g. 'esp32', 'all', 'global')" }
                },
                "required": ["trigger", "fix", "lesson"]
            })),
        ]
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> ToolResult {
        // Acquire global command lock for long-running tools
        let _lock = match name {
            "build_project" | "flash_project" | "build_flash_monitor" | "closed_loop" | "jtag_runtime_check" => {
                match crate::GlobalCommandLock::acquire(name) {
                    Ok(l) => Some(l),
                    Err(e) => return ok(json!({
                        "success": false,
                        "output": "",
                        "errors": [],
                        "error_count": 0,
                        "message": e
                    })),
                }
            }
            _ => None,
        };

        match name {
            "list_directory" => self.list_directory(args),
            "read_file" => self.read_file(args),
            "write_file" => self.write_file(args),
            "build_project" => self.build_project(args),
            "flash_project" => self.flash_project(args),
            "build_flash_monitor" => self.build_flash_monitor(args),
            "list_serial_ports" => self.list_serial_ports(),
            "detect_connection" => self.detect_connection_mcp(args),
            "get_connection_mode" => self.get_connection_mode_mcp(),
            "jtag_runtime_check" => self.jtag_runtime_check(args),
            "read_serial" => self.read_serial(args),
            "get_hardware_config" => self.get_hardware_config(),
            "export_hardware_header" => self.export_hardware_header(),
            "run_gdb_command" => self.run_gdb_command(args),
            "openocd_start" => self.openocd_start_mcp(args),
            "openocd_stop" => self.openocd_stop_mcp(),
            "openocd_is_running" => self.openocd_is_running_mcp(),
            "debug_start" => self.debug_start_mcp(args),
            "debug_stop" => self.debug_stop_mcp(),
            "debug_set_breakpoint" => self.debug_set_breakpoint_mcp(args),
            "debug_continue" => self.debug_continue_mcp(),
            "debug_step_over" => self.debug_step_over_mcp(),
            "debug_get_state" => self.debug_get_state_mcp(),
            "debug_read_variable" => self.debug_read_variable_mcp(args),
            "debug_get_registers" => self.debug_get_registers_mcp(),
            "debug_get_backtrace" => self.debug_get_backtrace_mcp(),
            "project_context" => self.project_context(),
            "closed_loop" => self.closed_loop(args),
            "query_experience" => self.query_experience(args),
            "record_experience" => self.record_experience(args),
            _ => err(format!("Unknown tool: {name}")),
        }
    }

    fn resolve_existing(&self, raw: &str) -> Result<PathBuf, String> {
        let joined = self.join_project_path(raw);
        let canonical = joined
            .canonicalize()
            .map_err(|e| format!("Invalid path '{raw}': {e}"))?;
        self.ensure_in_project(&canonical)?;
        Ok(canonical)
    }

    fn resolve_for_write(&self, raw: &str) -> Result<PathBuf, String> {
        let joined = self.join_project_path(raw);
        let parent = joined
            .parent()
            .ok_or_else(|| format!("Invalid path '{raw}'"))?;
        let canonical_parent = if parent.exists() {
            parent.canonicalize().map_err(|e| e.to_string())?
        } else {
            let nearest = nearest_existing_parent(parent)
                .ok_or_else(|| format!("No existing parent for '{raw}'"))?;
            nearest.canonicalize().map_err(|e| e.to_string())?
        };
        self.ensure_in_project(&canonical_parent)?;
        Ok(joined)
    }

    fn join_project_path(&self, raw: &str) -> PathBuf {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            self.project_root.join(path)
        }
    }

    fn ensure_in_project(&self, path: &Path) -> Result<(), String> {
        if path.starts_with(&self.project_root) {
            Ok(())
        } else {
            Err("Path outside project directory".to_string())
        }
    }

    fn idf_path(&self) -> Result<&str, String> {
        self.idf_path
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| "ESP-IDF path is not configured".to_string())
    }

    fn list_directory(&self, args: &Value) -> ToolResult {
        let raw = str_arg(args, "path").unwrap_or(".");
        let path = match self.resolve_existing(raw) {
            Ok(p) => p,
            Err(e) => return err(e),
        };
        let entries = match std::fs::read_dir(&path) {
            Ok(v) => v,
            Err(e) => return err(e.to_string()),
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let metadata = entry.metadata().ok();
            files.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "path": relative_path(&self.project_root, &entry.path()),
                "is_dir": metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                "size": metadata.map(|m| m.len()).unwrap_or(0)
            }));
        }
        ok(json!({ "entries": files }))
    }

    fn read_file(&self, args: &Value) -> ToolResult {
        let raw = match required_str_arg(args, "path") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let path = match self.resolve_existing(raw) {
            Ok(p) => p,
            Err(e) => return err(e),
        };
        if crate::commands::filesystem::is_binary_ext(&path) {
            return err(format!("Skipped binary file: {}", path.display()));
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => ok(json!({
                "path": relative_path(&self.project_root, &path),
                "content": content
            })),
            Err(e) => err(e.to_string()),
        }
    }

    fn write_file(&self, args: &Value) -> ToolResult {
        let raw = match required_str_arg(args, "path") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let content = match required_str_arg(args, "content") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let path = match self.resolve_for_write(raw) {
            Ok(p) => p,
            Err(e) => return err(e),
        };

        if is_protected_hardware_file(&path) {
            return err(
                "hardware_pins.h 是自动生成的文件，禁止直接修改。\n\
                 请通过修改 .espsmith/hardware_config.json 来更新硬件引脚配置。\n\
                 修改后 hardware_pins.h 会自动重新生成。"
                    .to_string(),
            );
        }

        if is_protected_toolchain_file(&path) {
            return err(
                "禁止修改工具链/OpenOCD 配置文件。\n\
                 这些文件属于 ESP-IDF 工具链，修改可能导致调试环境损坏。\n\
                 如果遇到 JTAG 连接问题，请检查芯片型号是否与项目配置一致。"
                    .to_string(),
            );
        }

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return err(e.to_string());
            }
        }
        match std::fs::write(&path, content) {
            Ok(_) => {
                let rel = relative_path(&self.project_root, &path);
                if is_hardware_config_json(&path) {
                    regenerate_hardware_pins(&self.project_root);
                }
                ok(json!({ "path": rel }))
            }
            Err(e) => err(e.to_string()),
        }
    }

    fn build_project(&self, args: &Value) -> ToolResult {
        let idf_path = match self.idf_path() {
            Ok(v) => v,
            Err(e) => return err(e),
        };

        // If target is specified, run idf.py set-target first
        if let Some(target) = str_arg(args, "target") {
            let set_result = crate::idf::run_idf_command_live(
                &self.project_root.to_string_lossy(),
                idf_path,
                &["set-target", target],
            );
            if let Err(e) = &set_result {
                return ok(json!({
                    "success": false,
                    "stage": "set-target",
                    "output": e,
                    "message": format!("Failed to set target to {}", target)
                }));
            }
        }

        let registry = adapters::create_idf_registry(idf_path);
        let ar = adapters::resolve_and_execute(
            &registry,
            "build.idf",
            &json!({}),
            &self.project_root.to_string_lossy(),
            idf_path,
        );
        if ar.success {
            let output = ar.stdout.unwrap_or_default();
            ok(json!({
                "success": true,
                "output": output,
                "errors": parse_compile_errors(&output)
            }))
        } else {
            let output = ar.stderr.clone().or(ar.stdout.clone()).unwrap_or_default();
            ok(json!({
                "success": false,
                "output": output,
                "errors": parse_compile_errors(&output)
            }))
        }
    }

    fn flash_project(&self, args: &Value) -> ToolResult {
        let idf_path = match self.idf_path() {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let port = match required_str_arg(args, "port") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let registry = adapters::create_idf_registry(idf_path);
        let ar = adapters::resolve_and_execute(
            &registry,
            "flash.idf_esptool",
            &json!({ "port": port }),
            &self.project_root.to_string_lossy(),
            idf_path,
        );
        if ar.success {
            ok(json!({ "success": true, "output": ar.stdout.unwrap_or_default() }))
        } else {
            let output = ar.stderr.clone().or(ar.stdout.clone()).unwrap_or_default();
            ok(json!({ "success": false, "output": output }))
        }
    }

    fn build_flash_monitor(&self, args: &Value) -> ToolResult {
        let port = match required_str_arg(args, "port") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let baudrate = u32_arg(args, "baudrate").unwrap_or(115200);
        let monitor_ms = u64_arg(args, "monitor_ms").unwrap_or(5000);

        let build = self.build_project(args);
        let build_ok = build
            .data
            .as_ref()
            .and_then(|v| v.get("success"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !build_ok {
            return build;
        }

        let flashed = self.flash_project(args);
        let flash_ok = flashed
            .data
            .as_ref()
            .and_then(|v| v.get("success"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !flash_ok {
            return flashed;
        }

        let serial = read_serial_once(port, baudrate, monitor_ms);
        match serial {
            Ok(output) => ok(json!({
                "success": true,
                "port": port,
                "baudrate": baudrate,
                "serial_output": output
            })),
            Err(e) => err(e),
        }
    }

    fn list_serial_ports(&self) -> ToolResult {
        let flash_port = crate::ai_assistant::get_cached_flash_port();
        let conn_info = connection::detect_connection_mode(flash_port.as_deref());
        match serialport::available_ports() {
            Ok(ports) => ok(json!({
                "ports": ports.into_iter().map(|p| {
                    let (vid, pid) = match &p.port_type {
                        serialport::SerialPortType::UsbPort(info) => (
                            Some(format!("{:04X}", info.vid)),
                            Some(format!("{:04X}", info.pid)),
                        ),
                        _ => (None, None),
                    };
                    let jtag_capable = vid.as_deref() == Some("303A");
                    let is_current = p.port_name == conn_info.port.as_deref().unwrap_or("");
                    json!({
                        "name": p.port_name,
                        "vid": vid,
                        "pid": pid,
                        "jtag_capable": jtag_capable,
                        "current_connection": is_current,
                    })
                }).collect::<Vec<_>>(),
                "detected_mode": conn_info.mode.as_str(),
                "detected_mode_label": conn_info.mode_label,
                "chip_hint": conn_info.chip_hint,
            })),
            Err(e) => err(e.to_string()),
        }
    }

    fn detect_connection_mcp(&self, args: &Value) -> ToolResult {
        let port = str_arg(args, "port");
        let info = connection::detect_connection_mode(port.map(|s| s as &str));
        match serde_json::to_value(&info) {
            Ok(val) => ok(val),
            Err(e) => err(e.to_string()),
        }
    }

    fn get_connection_mode_mcp(&self) -> ToolResult {
        let flash_port = crate::ai_assistant::get_cached_flash_port();
        let info = connection::detect_connection_mode(flash_port.as_deref());
        match serde_json::to_value(&info) {
            Ok(val) => ok(val),
            Err(e) => err(e.to_string()),
        }
    }

    fn jtag_runtime_check(&self, args: &Value) -> ToolResult {
        let port = match required_str_arg(args, "port") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let chip = match required_str_arg(args, "chip") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let baudrate = u32_arg(args, "baudrate").unwrap_or(115200);
        let monitor_ms = u64_arg(args, "monitor_ms").unwrap_or(5000);
        let expected = str_arg(args, "expected_pattern").unwrap_or("");
        let elf_path = str_arg(args, "elf_path")
            .map(|s| adapters::normalize_path_for_gdb(s).to_string())
            .or_else(|| find_elf_in_build_dir(&self.project_root.to_string_lossy()));

        let bp_args: Vec<String> = match args.get("breakpoints") {
            Some(Value::Array(arr)) => arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => vec![],
        };
        let watch_vars: Vec<String> = match args.get("watch_variables") {
            Some(Value::Array(arr)) => arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => vec![],
        };

        let start_total = Instant::now();
        let mut timeline: Vec<serde_json::Value> = vec![];

        let elf = match &elf_path {
            Some(p) => p.clone(),
            None => return err("No ELF file found. Specify elf_path or ensure build/ exists.".to_string()),
        };

        if !Path::new(&elf).exists() {
            return err(format!("ELF not found: {}", elf));
        }

        timeline.push(json!({"phase": "openocd_start", "ms": start_total.elapsed().as_millis()}));
        if let Err(e) = crate::commands::openocd::ensure_openocd_running(chip) {
            return err(format!("OpenOCD start failed: {}", e));
        }

        timeline.push(json!({"phase": "gdb_connect", "ms": start_total.elapsed().as_millis()}));
        if let Err(e) = crate::commands::gdb_session::connect_session_sync(
            &elf,
            "localhost:3333",
            chip,
        ) {
            crate::commands::openocd::kill_openocd_sync();
            return err(format!("GDB connect failed: {}", e));
        }

        let mut breakpoints_set = 0usize;
        let mut breakpoints_failed: Vec<String> = vec![];

        for bp in &bp_args {
            let bp_cmd = format!("break {}", bp);
            timeline.push(json!({"phase": "breakpoint_set", "location": bp, "ms": start_total.elapsed().as_millis()}));
            match crate::commands::gdb_session::send_mi_command_sync(bp_cmd.as_bytes()) {
                Ok(resp) => {
                    if resp.to_lowercase().contains("error") {
                        breakpoints_failed.push(format!("{} → {}", bp, resp));
                    } else {
                        breakpoints_set += 1;
                        timeline.push(json!({"phase": "breakpoint_ok", "location": bp, "response": resp, "ms": start_total.elapsed().as_millis()}));
                    }
                }
                Err(e) => {
                    breakpoints_failed.push(format!("{} → {}", bp, e));
                }
            }
        }

        timeline.push(json!({
            "phase": "breakpoints_summary",
            "set": breakpoints_set,
            "failed": breakpoints_failed.len(),
            "ms": start_total.elapsed().as_millis()
        }));

        timeline.push(json!({"phase": "continue", "ms": start_total.elapsed().as_millis()}));
        let _cont_resp = crate::commands::gdb_session::send_mi_command_sync(b"-exec-continue");

        std::thread::sleep(Duration::from_millis(200));

        let mut gdb_stopped_info = String::new();
        let mut gdb_stopped_pc = String::new();
        let mut gdb_hit_breakpoint = false;

        let probe_start = Instant::now();
        while probe_start.elapsed() < Duration::from_millis(600) {
            if let Ok(resp) = crate::commands::gdb_session::send_mi_command_sync(b"-thread-info") {
                if resp.to_lowercase().contains("stopped") {
                    let state = crate::commands::gdb_session::get_debug_state_sync();
                    gdb_stopped_info = state;
                    if let Ok(pc) = crate::commands::gdb_session::send_mi_command_sync(b"-data-evaluate-expression $pc") {
                        gdb_stopped_pc = pc;
                    }
                    gdb_hit_breakpoint = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        if gdb_hit_breakpoint {
            timeline.push(json!({
                "phase": "breakpoint_hit",
                "state": gdb_stopped_info,
                "pc": gdb_stopped_pc,
                "ms": start_total.elapsed().as_millis()
            }));

            if !watch_vars.is_empty() {
                let mut var_values = serde_json::Map::new();
                for var in &watch_vars {
                    let expr_cmd = format!("-data-evaluate-expression {}", var);
                    match crate::commands::gdb_session::send_mi_command_sync(expr_cmd.as_bytes()) {
                        Ok(val) => {
                            var_values.insert(var.clone(), json!(val));
                        }
                        Err(e) => {
                            var_values.insert(var.clone(), json!(format!("error: {}", e)));
                        }
                    }
                }
                timeline.push(json!({
                    "phase": "watch_variables",
                    "values": var_values,
                    "ms": start_total.elapsed().as_millis()
                }));
            }

            let mut reg_values = serde_json::Map::new();
            for reg_name in &["pc", "sp", "a0", "a1", "ra", "mie", "mstatus"] {
                let cmd = format!("-data-evaluate-expression ${}", reg_name);
                if let Ok(val) = crate::commands::gdb_session::send_mi_command_sync(cmd.as_bytes()) {
                    reg_values.insert(reg_name.to_string(), json!(val));
                }
            }
            timeline.push(json!({
                "phase": "registers_snapshot",
                "values": reg_values,
                "ms": start_total.elapsed().as_millis()
            }));

            timeline.push(json!({"phase": "continue_after_bp", "ms": start_total.elapsed().as_millis()}));
            let _ = crate::commands::gdb_session::send_mi_command_sync(b"-exec-continue");
        }

        timeline.push(json!({"phase": "serial_monitor_start", "ms": start_total.elapsed().as_millis()}));
        let serial_output = read_serial_once(port, baudrate, monitor_ms).unwrap_or_default();
        timeline.push(json!({
            "phase": "serial_captured",
            "length": serial_output.len(),
            "ms": start_total.elapsed().as_millis()
        }));

        let crash = detect_crash_patterns(&serial_output);
        if !crash.is_empty() {
            timeline.push(json!({"phase": "crash_detected", "type": crash, "ms": start_total.elapsed().as_millis()}));

            let gdb_state = read_gdb_crash_state(&elf, chip).unwrap_or_default();
            timeline.push(json!({"phase": "gdb_crash_state_captured", "ms": start_total.elapsed().as_millis()}));

            crate::commands::gdb_session::disconnect_session_sync();
            crate::commands::openocd::kill_openocd_sync();

            return ok(json!({
                "result": "crash",
                "crash_type": crash,
                "serial_output": serial_output,
                "gdb_crash_state": gdb_state,
                "timeline": timeline,
                "breakpoints_set": breakpoints_set,
                "breakpoints_hit": gdb_hit_breakpoint,
                "total_ms": start_total.elapsed().as_millis(),
            }));
        }

        let pattern_match = expected.is_empty() || serial_output.contains(expected);

        timeline.push(json!({"phase": "pattern_check", "matched": pattern_match, "expected": expected, "ms": start_total.elapsed().as_millis()}));

        crate::commands::gdb_session::disconnect_session_sync();
        crate::commands::openocd::kill_openocd_sync();

        ok(json!({
            "result": if pattern_match { "pass" } else { "pattern_mismatch" },
            "serial_output": serial_output,
            "expected_pattern": expected,
            "pattern_matched": pattern_match,
            "breakpoints_set": breakpoints_set,
            "breakpoints_hit": gdb_hit_breakpoint,
            "breakpoints_failed": breakpoints_failed,
            "watch_variables_requested": watch_vars,
            "timeline": timeline,
            "total_ms": start_total.elapsed().as_millis(),
        }))
    }

    fn read_serial(&self, args: &Value) -> ToolResult {
        let port = match required_str_arg(args, "port") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let baudrate = u32_arg(args, "baudrate").unwrap_or(115200);
        let duration_ms = u64_arg(args, "duration_ms").unwrap_or(3000);
        match read_serial_once(port, baudrate, duration_ms) {
            Ok(output) => ok(json!({ "port": port, "baudrate": baudrate, "output": output })),
            Err(e) => err(e),
        }
    }

    fn get_hardware_config(&self) -> ToolResult {
        let path = self.project_root.join(".espsmith").join("hardware_config.json");
        if !path.exists() {
            return ok(json!({ "peripherals": {} }));
        }
        match std::fs::read_to_string(&path).and_then(|s| {
            serde_json::from_str::<Value>(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }) {
            Ok(config) => ok(config),
            Err(e) => err(e.to_string()),
        }
    }

    fn export_hardware_header(&self) -> ToolResult {
        let config = match self.get_hardware_config().data {
            Some(v) => v,
            None => return err("No hardware config".to_string()),
        };
        let mut header = String::from(
            "// Auto-generated by EspSmith\n#ifndef HARDWARE_CONFIG_H\n#define HARDWARE_CONFIG_H\n\n",
        );
        if let Some(peripherals) = config.get("peripherals").and_then(|v| v.as_object()) {
            for (id, peripheral) in peripherals {
                let safe = id.to_uppercase().replace('-', "_");
                if let Some(pins) = peripheral.get("pin_values").and_then(|v| v.as_object()) {
                    for (pin_name, pin_value) in pins {
                        header.push_str(&format!(
                            "#define {}_{}_GPIO {}\n",
                            safe,
                            pin_name.to_uppercase(),
                            pin_value
                        ));
                    }
                    header.push('\n');
                }
            }
        }
        header.push_str("#endif // HARDWARE_CONFIG_H\n");
        ok(json!({ "content": header }))
    }

    fn run_gdb_command(&self, args: &Value) -> ToolResult {
        let command = match required_str_arg(args, "command") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let board = self.detect_board();
        let gdb_binary = match crate::commands::gdb_session::find_gdb_binary(Some(&board)) {
            Ok(path) => path,
            Err(e) => return err(format!("GDB not found: {e}")),
        };
        let output = std::process::Command::new(&gdb_binary)
            .args(["-batch", "-nx", "-ex", "target remote localhost:3333", "-ex", command])
            .current_dir(&self.project_root)
            .output();
        match output {
            Ok(out) => ok(json!({
                "success": out.status.success(),
                "stdout": String::from_utf8_lossy(&out.stdout),
                "stderr": String::from_utf8_lossy(&out.stderr)
            })),
            Err(e) => err(format!("GDB failed ({gdb_binary}): {e}")),
        }
    }

    fn project_context(&self) -> ToolResult {
        let flash_port = crate::ai_assistant::get_cached_flash_port();
        let conn_info = connection::detect_connection_mode(flash_port.as_deref());
        let is_jtag = conn_info.mode.is_jtag();
        let mode_str = conn_info.mode.as_str();
        let mode_label = &conn_info.mode_label;
        let recommendation = &conn_info.recommendation;
        let chip_hint = conn_info.chip_hint.as_deref().unwrap_or("unknown");
        let configured_chip = self.detect_board();
        let chip_mismatch = conn_info.chip_hint.as_deref().is_some_and(|h| {
            let hint = h.to_ascii_lowercase().replace('-', "");
            hint != "esp32usbjtag" && hint != configured_chip.to_ascii_lowercase().replace('-', "")
        });
        let detected_port = conn_info.port.clone().unwrap_or_else(|| "COM3".to_string());

        let mut workflow = vec![
            "1. Edit code: write_file if user explicitly asks for code changes.",
            "2. Build: call build_project() without target. Only pass {target: \"chip\"} if the user explicitly asks to change the chip target, because set-target triggers a full reconfiguration and is very slow. Check errors array if failed, fix code, rebuild.",
        ];

        if chip_mismatch {
            workflow.push("! CHIP MISMATCH WARNING: chip_hint (USB PID) differs from configured_chip (project config).");
            workflow.push("  The connected USB device may NOT match the project's target chip. If OpenOCD reports TAP ID mismatch, this is the likely cause.");
            workflow.push("  DO NOT modify OpenOCD config files. Verify the physical chip model. If the chip is correct, ignore this warning.");
        }

        if is_jtag {
            workflow.push("3. [JTAG MODE - RECOMMENDED] Quick verify: call closed_loop(port=\"PORT\") using detected_port below — OpenOCD flash + serial output check + GDB PC/stack verification.");
            workflow.push("4. [JTAG DEEP CHECK] If you need to trace execution at runtime: call jtag_runtime_check(port=\"PORT\", chip=\"CHIP\", breakpoints=[\"app_main\"], watch_variables=[\"counter\"])");
            workflow.push("   This sets hardware breakpoints, runs the program, captures variable values at breakpoints, reads serial output, and auto-captures GDB state on crash.");
            workflow.push("   JTAG benefits: hardware breakpoints, register view, backtrace on crash, no serial port lock.");
        } else {
            workflow.push("3. [UART MODE] Flash & verify: call closed_loop(port=\"PORT\") using detected_port — uses esptool serial flash + serial output verification.");
            workflow.push("   Tip: switch to a USB-JTAG capable chip (ESP32-S3/C3/C6/H2) for deeper debugging.");
        }

        workflow.push("Connection mode is already detected & cached. Use get_connection_mode if you need to verify — NO need to call detect_connection unless the device just changed.");
        workflow.push("NEVER use exec_shell, run_command, dir, type, echo, cat, ls. Use MCP tools only.");
        workflow.push("NEVER run openocd.exe directly (it blocks forever). Use openocd_start tool or espsmith.exe openocd-start instead.");
        workflow.push("JTAG DIAGNOSTIC: If OpenOCD fails with 'TAP ID mismatch' or 'Unsupported DTM version', the physical chip does NOT match the project target. DO NOT modify OpenOCD config files — instead, verify the actual chip model on the board and update project config if needed.");
        workflow.push("KNOWN JTAG IDs: ESP32 (Xtensa) = 0x120034e5, ESP32-S3 (Xtensa) = 0x120034e5, ESP32-C3 (RISC-V) = 0x00005c25. If you see a Tensilica/Xtensa ID on a RISC-V project, the connected chip is NOT a RISC-V chip.");
        workflow.push("NOTE: chip_hint = 'ESP32-USB-JTAG' means the chip has built-in USB-JTAG (S3/C3/C6/H2/etc.) but the exact model cannot be determined from USB PID alone. Trust the project config.");

        ok(json!({
            "project_root": self.project_root,
            "idf_path": self.idf_path,
            "idf_version": self.idf_path.as_deref().map(crate::idf::get_idf_version),
            "configured_chip": configured_chip,
            "detected_port": detected_port,
            "chip_mismatch": chip_mismatch,
            "connection": {
                "mode": mode_str,
                "mode_label": mode_label,
                "is_jtag": is_jtag,
                "jtag_recommended": is_jtag,
                "chip_hint": chip_hint,
                "capabilities": conn_info.capabilities,
                "recommendation": recommendation,
            },
            "available_tools": {
                "detect_connection": "[RARELY NEEDED — connection is already cached] Re-scan USB-JTAG vs UART. Use get_connection_mode instead for cached result.",
                "get_connection_mode": "Get cached connection mode from last detection. USE THIS by default.",
                "build_project": "Execute idf.py build. Do NOT pass target unless changing chip (set-target triggers full reconfiguration). Returns { success, output, errors: [{file, line, column, type, message}] }. Call this after editing code.",
                "flash_project": "Flash firmware to serial port (UART only). Args: { port: string }. For JTAG use closed_loop.",
                "closed_loop": "ONE-CLICK build+flash+verify. Uses cached connection mode (JTAG → OpenOCD flash + GDB PC/stack check + serial; UART → esptool + serial). Args: { port: string, board?: string, expected_pattern?: string, force_jtag?: bool, force_uart?: bool }.",
                "jtag_runtime_check": "DEEP JTAG RUNTIME CHECK (only for breakpoints/watch-variables). For general verification use closed_loop instead. If jtag_runtime_check fails, fall back to closed_loop — do NOT manually invoke GDB. Args: { port, chip, elf_path?, breakpoints?: string[], watch_variables?: string[], expected_pattern?: string }.",
                "build_flash_monitor": "Build + flash + read serial in one step (UART only). Args: { port: string, baudrate?: int, monitor_ms?: int }.",
                "list_serial_ports": "List available COM ports with JTAG detection. Returns { ports: [{name, vid, pid, jtag_capable?}] }.",
                "read_serial": "Read serial output. Args: { port: string, baudrate?: int, duration_ms?: int }.",
                "read_file": "Read a project file. Args: { path: string }.",
                "write_file": "Write a project file. Args: { path: string, content: string }. Creates parent dirs.",
                "list_directory": "List project directory. Args: { path?: string }.",
                "get_hardware_config": "Read .espsmith/hardware_config.json.",
                "export_hardware_header": "Generate hardware_config.h from config.",
                "openocd_start": "Start OpenOCD JTAG server. Args: { chip?: string }.",
                "openocd_stop": "Stop OpenOCD.",
                "openocd_is_running": "Check if OpenOCD is running.",
                "run_gdb_command": "Run GDB batch command. Args: { command: string }.",
                "debug_start": "Start persistent GDB session. Args: { elf_path: string, target?: string, target_chip?: string }.",
                "debug_stop": "Stop GDB session.",
                "debug_get_state": "Get debug state (PC, stack, registers).",
                "debug_get_backtrace": "Get call stack (backtrace).",
                "debug_set_breakpoint": "Set breakpoint. Args: { file: string, line: int }.",
                "debug_continue": "Continue execution.",
                "debug_step_over": "Step over current line."
            },
            "workflow": workflow,
            "forbidden": [
                "exec_shell / run_command with idf.py, export.bat, install.bat, pip, dir, type, echo, cat, ls",
                "Any shell command that could be done by MCP tools above",
                "Installing, repairing, or modifying ESP-IDF toolchain",
                "Directly modifying hardware_pins.h (auto-generated from hardware_config.json)",
                "Modifying OpenOCD config files (.cfg) or ESP-IDF toolchain files — these are system tools, not project files",
                "Modifying any file outside the project directory"
            ]
        }))
    }

    /// 从项目配置中自动检测目标芯片号
    fn detect_board(&self) -> String {
        // 尝试读取 .espsmith/project.json
        let config_path = self.project_root.join(".espsmith").join("project.json");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(chip) = cfg["chipModel"].as_str().or_else(|| cfg["chip"].as_str()) {
                    return chip.to_string();
                }
            }
        }
        // 尝试读取 sdkconfig.defaults 中的 CONFIG_IDF_TARGET
        let sdkconfig = self.project_root.join("sdkconfig.defaults");
        if let Ok(content) = std::fs::read_to_string(&sdkconfig) {
            for line in content.lines() {
                if let Some(target) = line.strip_prefix("CONFIG_IDF_TARGET=\"") {
                    if let Some(end) = target.find('"') {
                        return target[..end].to_string();
                    }
                }
            }
        }
        "esp32".into()
    }

    fn closed_loop(&self, args: &Value) -> ToolResult {
        let port = match required_str_arg(args, "port") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let baudrate = u32_arg(args, "baudrate").unwrap_or(115200);
        let monitor_ms = u64_arg(args, "monitor_ms").unwrap_or(5000);
        let expected = str_arg(args, "expected_pattern").unwrap_or("");

        let board = str_arg(args, "board")
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.detect_board());

        let force_jtag = bool_arg(args, "force_jtag").unwrap_or(false);
        let force_uart = bool_arg(args, "force_uart").unwrap_or(false);
        let elf_path = str_arg(args, "elf_path").map(|s| adapters::normalize_path_for_gdb(s).to_string());
        let expected_pc_mask = str_arg(args, "expected_pc_mask").unwrap_or("0x40000000");

        let flash_port_for_conn = crate::ai_assistant::get_cached_flash_port();
        let conn_info = connection::detect_connection_mode(flash_port_for_conn.as_deref());
        let is_jtag = if force_jtag {
            true
        } else if force_uart {
            false
        } else if conn_info.mode.is_jtag() {
            true
        } else {
            let detected = connection::detect_connection_mode(Some(port));
            tracing::info!(
                "closed_loop: cached mode={:?}, re-detected mode={:?}",
                conn_info.mode,
                detected.mode
            );
            detected.mode.is_jtag()
        };
        let connection_label = if is_jtag { "JTAG (USB-JTAG)" } else { "UART (Serial)" };

        let chip_mismatch = if is_jtag {
            let hint = conn_info.chip_hint.as_ref()
                .map(|h| h.to_ascii_lowercase().replace('-', ""));
            hint.is_some_and(|h| h != "esp32usbjtag" && h != board.to_ascii_lowercase().replace('-', ""))
        } else {
            false
        };
        if chip_mismatch {
            tracing::warn!(
                "closed_loop: chip mismatch — configured={}, USB hint={}. JTAG may fail.",
                board,
                conn_info.chip_hint.as_deref().unwrap_or("unknown")
            );
        }

        let idf = self.idf_path.clone().unwrap_or_default();
        let registry = adapters::create_idf_registry(&idf);

        let mut steps = vec![
            self_healing::stages::preflight(&board, port),
            self_healing::stages::build("idf", &[]),
        ];

        if is_jtag {
            steps.push(self_healing::stages::openocd_flash(&board, port));
            steps.push(self_healing::stages::serial_verify(port, baudrate, expected));
            steps.push(self_healing::stages::gdb_session_verify(expected_pc_mask, None, 1, &board));
        } else {
            steps.push(self_healing::stages::flash(&board, port));
            steps.push(self_healing::stages::serial_verify(port, baudrate, expected));
        }

        let gdb_verify_enabled = is_jtag;

        let plan = Plan {
            name: format!("closed_loop_{}", if is_jtag { "jtag" } else { "uart" }),
            board: board.clone(),
            test: "verify".into(),
            steps,
            recovery_policy: if is_jtag {
                RecoveryPolicy::full()
            } else {
                RecoveryPolicy::default()
            },
            timeout_s: Some(300.0),
            guard_limit: Some(5),
        };

        if gdb_verify_enabled {
            if let Some(ref elf) = elf_path {
                self_healing::recovery::set_gdb_recovery_context((*elf).to_string(), board.to_string());
            }
        }

        let project_root = self.project_root.to_string_lossy().to_string();
        let idf_path = idf.clone();
        let board_clone = board.clone();
        let elf_path_clone = elf_path.clone();
        let port_clone = port.to_string();

        #[allow(clippy::type_complexity)]
        let sink_arc: Option<Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync>> = self.progress_sink.as_ref().map(|s| s.clone());        let collected_events: std::sync::Arc<std::sync::Mutex<Vec<RunnerEvent>>> =             std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));        let collected_events_clone = collected_events.clone();        let result = runner::run_plan_with_progress(&plan, &|step, ctx| {
            let start = Instant::now();

            if step.adapter.starts_with("verify.serial") {
                match read_serial_once(&port_clone, baudrate, monitor_ms) {
                    Ok(out) => {
                        let crash = detect_crash_patterns(&out);
                        if !crash.is_empty() {
                            let mut report = format!(
                                "CRASH DETECTED [{} mode]\nCrash type: {}\n\nSerial output:\n{}\n",
                                connection_label, crash, out
                            );

                            if gdb_verify_enabled && ctx.get("flash_ok").map(|v| v.as_str()) == Some("true") {
                                let _ = crate::commands::openocd::ensure_openocd_running(&board_clone);
                                if let Some(ref elf) = elf_path_clone {
                                    if crate::commands::gdb_session::connect_session_sync(
                                        elf,
                                        "localhost:3333",
                                        &board_clone,
                                    ).is_ok() {
                                        report.push_str("\n--- GDB Crash State (auto-captured) ---\n");
                                        match read_gdb_crash_state(elf, &board_clone) {
                                            Ok(state) => report.push_str(&state),
                                            Err(e) => report.push_str(&format!("GDB state read failed: {}\n", e)),
                                        }
                                        crate::commands::gdb_session::disconnect_session_sync();
                                    }
                                }
                            }
                            Ok(StepResult::failed(&step.name, 1, report, start.elapsed().as_millis() as u64))
                        } else if expected.is_empty() || out.contains(expected) {
                            Ok(StepResult::passed(&step.name, 1, start.elapsed().as_millis() as u64))
                        } else {
                            Ok(StepResult::failed(&step.name, 1,
                                format!("Pattern '{}' not found in: {}", expected, out),
                                start.elapsed().as_millis() as u64))
                        }
                    }
                    Err(e) => Ok(StepResult::failed(&step.name, 1, format!("Serial error: {}", e), start.elapsed().as_millis() as u64)),
                }
            } else if step.adapter == "verify.gdb_session" {
                let _ = crate::commands::openocd::ensure_openocd_running(&board_clone);
                std::thread::sleep(std::time::Duration::from_millis(500));
                if crate::commands::gdb_session::GDB_SESSION.lock().map_or(true, |g| g.is_none()) {
                    if let Some(ref elf) = elf_path_clone {
                        match crate::commands::gdb_session::connect_session_sync(
                            elf,
                            "localhost:3333",
                            &board_clone,
                        ) {
                            Ok(()) => {
                                tracing::info!("GDB session re-connected for verify.gdb_session");
                            }
                            Err(e) => {
                                tracing::warn!("GDB session connect failed for verify: {}", e);
                            }
                        }
                    }
                }
                let ar = adapters::resolve_and_execute(
                    &registry,
                    &step.adapter,
                    &step.params,
                    &project_root,
                    &idf_path,
                );
                Ok(if ar.success {
                    StepResult::passed(&step.name, 1, ar.duration_ms)
                } else {
                    let msg = ar.error.unwrap_or_else(|| "Unknown error".into());
                    StepResult::failed(&step.name, 1, msg, ar.duration_ms)
                })
            } else if step.adapter.starts_with("flash.") {
                let ar = adapters::resolve_and_execute(
                    &registry,
                    &step.adapter,
                    &step.params,
                    &project_root,
                    &idf_path,
                );

                if ar.success {
                    ctx.insert("flash_ok".into(), "true".into());

                    if gdb_verify_enabled {
                        let _ = crate::commands::openocd::ensure_openocd_running(&board_clone);
                        if let Some(ref elf) = elf_path_clone {
                            match crate::commands::gdb_session::connect_session_sync(
                                elf,
                                "localhost:3333",
                                &board_clone,
                            ) {
                                Ok(()) => {
                                    tracing::info!("GDB session connected for verify step");
                                }
                                Err(e) => {
                                    tracing::error!("GDB session connect failed: {}", e);
                                }
                            }
                        }
                    }
                }

                Ok(if ar.success {
                    StepResult::passed(&step.name, 1, ar.duration_ms)
                } else {
                    let msg = ar.error.unwrap_or_else(|| "Unknown error".into());
                    StepResult::failed(&step.name, 1, msg, ar.duration_ms)
                })
            } else {
                let ar = adapters::resolve_and_execute(
                    &registry,
                    &step.adapter,
                    &step.params,
                    &project_root,
                    &idf_path,
                );
                Ok(if ar.success {
                    StepResult::passed(&step.name, 1, ar.duration_ms)
                } else {
                    let msg = ar.error.unwrap_or_else(|| "Unknown error".into());
                    StepResult::failed(&step.name, 1, msg, ar.duration_ms)
                })
            }
        }, &move |event| {
            collected_events_clone.lock().unwrap().push(event.clone());
            if let Some(sink) = sink_arc.as_ref() {
                sink(event);
            }
        });

        if gdb_verify_enabled {
            crate::commands::gdb_session::disconnect_session_sync();
            crate::commands::openocd::kill_openocd_sync();
            self_healing::recovery::clear_gdb_recovery_context();
        }

        experience::record_run(&board, "verify", result.passed);

        // 只在闭环失败且触发了自愈修复时才记录 skill（真正的疑难杂症），
        // 成功的运行只更新 stats，不生成无价值的流水账记录。
        if !result.passed && !result.recovery_applied.is_empty() {
            let record = experience::ExperienceRecord {
                id: format!("{}_{}_{}", plan.name, board, chrono::Utc::now().timestamp()),
                trigger: format!("closed_loop_{}_failed", if is_jtag { "jtag" } else { "uart" }),
                fix: format!("Self-healing applied on {} ({}: {:?})", board, connection_label, conn_info.mode),
                lesson: format!("{} steps: {} pass / {} fail. Recovery: {}",
                    result.total_steps, result.passed_steps, result.total_steps - result.passed_steps,
                    result.recovery_applied.join("; ")),
                scope: board.clone(),
                board_id: None,
                source_ref: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            experience::record_skill(record);
        }

        ok(json!({
            "passed": result.passed,
            "total_steps": result.total_steps,
            "passed_steps": result.passed_steps,
            "total_attempts": result.total_attempts,
            "duration_ms": result.total_duration_ms,
            "recovery_applied": result.recovery_applied,            "runner_events": collected_events.lock().unwrap().iter().cloned().collect::<Vec<_>>(),
            "summary": result.summary,
            "connection_mode": conn_info.mode.as_str(),
            "connection_label": connection_label,
            "jtag_detected": conn_info.mode.is_jtag(),
            "jtag_used": is_jtag,
            "chip_hint": conn_info.chip_hint,
            "chip_mismatch": chip_mismatch,
            "capabilities": conn_info.capabilities,
            "step_details": result.step_results.iter().map(|sr| json!({
                "step": sr.step_name,
                "status": format!("{:?}", sr.status),
                "attempt": sr.attempt,
                "duration_ms": sr.duration_ms,
                "error": sr.error,
            })).collect::<Vec<_>>(),
        }))
    }

    fn query_experience(&self, args: &Value) -> ToolResult {
        let board = match required_str_arg(args, "board") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let test = str_arg(args, "test").unwrap_or("verify");

        match experience::query_context(board, test) {
            Some(ctx) => ok(serde_json::to_value(&ctx).unwrap_or_else(|_| json!({}))),
            None => ok(json!({
                "available": false,
                "message": format!("No experience data for board '{}'. Run closed_loop first to start accumulating experience.", board)
            })),
        }
    }

    fn record_experience(&self, args: &Value) -> ToolResult {
        let trigger = match required_str_arg(args, "trigger") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let fix = match required_str_arg(args, "fix") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let lesson = match required_str_arg(args, "lesson") {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let scope = str_arg(args, "scope").unwrap_or("all");

        let record = experience::engine::ExperienceRecord {
            id: format!("skill_{}", chrono_id()),
            trigger: trigger.to_string(),
            fix: fix.to_string(),
            lesson: lesson.to_string(),
            scope: scope.to_string(),
            board_id: None,
            source_ref: None,
            timestamp: chrono_timestamp(),
        };

        if experience::record_skill(record) {
            ok(json!({ "recorded": true, "message": "Experience recorded successfully" }))
        } else {
            err("Experience engine not initialized. Start an AI session first.".into())
        }
    }

    fn openocd_start_mcp(&self, args: &Value) -> ToolResult {
        let chip = str_arg(args, "chip");
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::openocd::openocd_start(chip.map(|s| s.to_string()), None).await
        });
        match result {
            Ok(msg) => ok(json!({ "started": true, "message": msg })),
            Err(e) => err(e),
        }
    }

    fn openocd_stop_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::openocd::openocd_stop().await
        });
        match result {
            Ok(msg) => ok(json!({ "stopped": true, "message": msg })),
            Err(e) => err(e),
        }
    }

    fn openocd_is_running_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::openocd::openocd_is_running().await
        });
        match result {
            Ok(running) => ok(json!({ "running": running })),
            Err(e) => err(e),
        }
    }

    fn debug_start_mcp(&self, args: &Value) -> ToolResult {
        let elf = str_arg(args, "elf_path");
        let target = str_arg(args, "target");
        let chip = str_arg(args, "target_chip");
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_start(
                elf.map(|s| s.to_string()),
                target.map(|s| s.to_string()),
                chip.map(|s| s.to_string()),
            ).await
        });
        match result {
            Ok(state) => ok(serde_json::to_value(&state).unwrap_or(json!({}))),
            Err(e) => err(e),
        }
    }

    fn debug_stop_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_stop().await
        });
        match result {
            Ok(_) => ok(json!({ "stopped": true })),
            Err(e) => err(e),
        }
    }

    fn debug_set_breakpoint_mcp(&self, args: &Value) -> ToolResult {
        let file = match required_str_arg(args, "file") { Ok(v) => v, Err(e) => return err(e), };
        let line = match args.get("line").and_then(|v| v.as_u64()) {
            Some(v) => v as u32,
            None => return err("Missing required parameter 'line'".into()),
        };
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_set_breakpoint(file.to_string(), line).await
        });
        match result {
            Ok(bp) => ok(serde_json::to_value(&bp).unwrap_or(json!({}))),
            Err(e) => err(e),
        }
    }

    fn debug_continue_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_continue().await
        });
        match result {
            Ok(state) => ok(serde_json::to_value(&state).unwrap_or(json!({}))),
            Err(e) => err(e),
        }
    }

    fn debug_step_over_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_step_over().await
        });
        match result {
            Ok(state) => ok(serde_json::to_value(&state).unwrap_or(json!({}))),
            Err(e) => err(e),
        }
    }

    fn debug_get_state_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_get_state().await
        });
        match result {
            Ok(state) => ok(serde_json::to_value(&state).unwrap_or(json!({}))),
            Err(e) => err(e),
        }
    }

    fn debug_read_variable_mcp(&self, args: &Value) -> ToolResult {
        let name = match required_str_arg(args, "name") { Ok(v) => v, Err(e) => return err(e), };
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_read_variable(name.to_string()).await
        });
        match result {
            Ok(info) => ok(serde_json::to_value(&info).unwrap_or(json!({}))),
            Err(e) => err(e),
        }
    }

    fn debug_get_registers_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_get_registers().await
        });
        match result {
            Ok(regs) => ok(json!({ "registers": regs })),
            Err(e) => err(e),
        }
    }

    fn debug_get_backtrace_mcp(&self) -> ToolResult {
        let result = tokio::runtime::Handle::current().block_on(async {
            crate::commands::gdb_session::debug_get_backtrace().await
        });
        match result {
            Ok(frames) => ok(serde_json::to_value(&frames).unwrap_or(json!([]))),
            Err(e) => err(e),
        }
    }
}


/// Back-compat wrapper for callers that do not care about runner events.
pub fn call_tool_direct(
    project_root: String,
    idf_path: Option<String>,
    tool_name: &str,
    args: &Value,
) -> ToolResult {
    call_tool_direct_with_progress(project_root, idf_path, tool_name, args, None)
}

/// Run a single tool with an optional runner-event sink. The sink is
/// attached to the `MCPServer` instance used for this call so that tools
/// like `closed_loop` can stream `StepStarted` / `StepFailed` /
/// `RecoveryApplied` events to the caller in real time.
#[allow(clippy::type_complexity)]
pub fn call_tool_direct_with_progress(
    project_root: String,
    idf_path: Option<String>,
    tool_name: &str,
    args: &Value,
    progress_sink: Option<Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync>>,
) -> ToolResult {
    let server = MCPServer::new(project_root, idf_path).map_err(|e| e.to_string());
    match server {
        Ok(server) => {
            let server = match progress_sink {
                Some(s) => server.with_progress_sink(s),
                None => server,
            };
            server.call_tool(tool_name, args)
        }
        Err(e) => ToolResult { success: false, data: None, error: Some(e) },
    }
}


pub fn run_stdio_server() -> Result<(), String> {
    let server = MCPServer::from_env()?;
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();

    while let Some(request) = read_mcp_message(&mut reader)? {
        let Some(response) = handle_jsonrpc_request(&server, request) else {
            continue;
        };
        write_mcp_message(&mut stdout, &response)?;
    }

    Ok(())
}

fn handle_jsonrpc_request(server: &MCPServer, request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");

    let id = id?;

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "espsmith", "version": env!("CARGO_PKG_VERSION") }
        }),
        "tools/list" => json!({ "tools": server.list_tools() }),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let result = server.call_tool(name, &args);
            json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string())
                }],
                "isError": !result.success
            })
        }
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {method}") }
            }));
        }
    };

    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn read_mcp_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, String> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(
                value.trim().parse::<usize>().map_err(|e| format!("Bad Content-Length: {e}"))?,
            );
        } else if trimmed.starts_with('{') {
            return serde_json::from_str(trimmed)
                .map(Some)
                .map_err(|e| format!("Bad JSON-RPC message: {e}"));
        }
    }

    let len = match content_length {
        Some(v) => v,
        None => return Err("Missing Content-Length".to_string()),
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| format!("Bad JSON-RPC body: {e}"))
}

fn write_mcp_message<W: Write>(writer: &mut W, response: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(response).map_err(|e| e.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
    writer.write_all(&body).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

fn read_serial_once(port: &str, baudrate: u32, duration_ms: u64) -> Result<String, String> {
    let mut serial = serialport::new(port, baudrate)
        .timeout(Duration::from_millis(100))
        .open()
        .map_err(|e| format!("Failed to open serial port: {e}"))?;
    let deadline = Instant::now() + Duration::from_millis(duration_ms);
    let mut output = Vec::new();
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        match serial.read(&mut buf) {
            Ok(n) if n > 0 => output.extend_from_slice(&buf[..n]),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(String::from_utf8_lossy(&output).to_string())
}

fn parse_compile_errors(output: &str) -> Vec<Value> {
    let re = regex::Regex::new(r"([^:\r\n]+):(\d+):(\d+):\s+(error|warning):\s+(.+)").unwrap();
    output
        .lines()
        .filter_map(|line| {
            let caps = re.captures(line)?;
            Some(json!({
                "file": caps.get(1).map(|m| m.as_str()).unwrap_or(""),
                "line": caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0),
                "column": caps.get(3).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0),
                "type": caps.get(4).map(|m| m.as_str()).unwrap_or("error"),
                "message": caps.get(5).map(|m| m.as_str()).unwrap_or("")
            }))
        })
        .collect()
}

fn nearest_existing_parent(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn tool(name: &str, description: &str, input_schema: Value) -> Tool {
    Tool {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
    }
}

fn ok(data: Value) -> ToolResult {
    ToolResult {
        success: true,
        data: Some(data),
        error: None,
    }
}

fn err(error: String) -> ToolResult {
    ToolResult {
        success: false,
        data: None,
        error: Some(error),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn required_str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("Missing argument: {key}"))
}

fn u32_arg(args: &Value, key: &str) -> Option<u32> {
    args.get(key).and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok())
}

fn u64_arg(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| v.as_u64())
}

fn bool_arg(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|v| v.as_bool())
}

fn chrono_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{:x}", ms)
}

const CRASH_PATTERNS: &[&str] = &[
    "Guru Meditation Error",
    "abort() was called",
    "assert failed:",
    "PANIC",
    "Backtrace:",
    "Rebooting...",
    "LoadProhibited",
    "StoreProhibited",
    "IllegalInstruction",
    "DivideByZero",
    "Stack canary watchpoint triggered",
    "Brownout",
    "Core  0 register dump",
    "Core  1 register dump",
    "rst:",
];

fn detect_crash_patterns(serial_output: &str) -> String {
    let mut found = Vec::new();
    for pattern in CRASH_PATTERNS {
        if serial_output.contains(pattern) {
            found.push(*pattern);
        }
    }
    if found.is_empty() {
        return String::new();
    }
    format!("Detected crash signatures: {}", found.join(", "))
}

fn read_gdb_crash_state(elf_path: &str, board: &str) -> Result<String, String> {
    let mut report = String::new();

    if let Err(e) = crate::commands::gdb_session::connect_session_sync(
        elf_path,
        "localhost:3333",
        board,
    ) {
        return Err(format!("Failed to connect GDB for crash state: {}", e));
    }

    match crate::commands::gdb_session::gdb_send_command("-stack-list-frames 0 20") {
        Ok(raw) => {
            report.push_str("Backtrace:\n");
            for line in raw.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("^done") {
                    if let Some(stack) = crate::commands::gdb_session::extract_mi_list(trimmed, "stack") {
                        for frame_str in crate::commands::gdb_session::split_mi_values(&stack) {
                            let level = crate::commands::gdb_session::extract_mi_field(frame_str, "level")
                                .unwrap_or_else(|| "?".into());
                            let func = crate::commands::gdb_session::extract_mi_field(frame_str, "func")
                                .unwrap_or_else(|| "?".into());
                            let file = crate::commands::gdb_session::extract_mi_field(frame_str, "fullname")
                                .or_else(|| crate::commands::gdb_session::extract_mi_field(frame_str, "file"))
                                .unwrap_or_else(|| "?".into());
                            let line_num = crate::commands::gdb_session::extract_mi_field(frame_str, "line")
                                .unwrap_or_else(|| "?".into());
                            report.push_str(&format!("  #{} {} at {}:{}\n", level, func, file, line_num));
                        }
                    }
                }
            }
        }
        Err(e) => {
            report.push_str(&format!("Backtrace failed: {}\n", e));
        }
    }

    match crate::commands::gdb_session::gdb_send_mi("-data-evaluate-expression \"$pc\"") {
        Ok(resp) => {
            if let Some(pc) = crate::commands::gdb_session::extract_mi_field(&resp, "value") {
                report.push_str(&format!("PC: {}\n", pc));
            }
        }
        Err(e) => {
            report.push_str(&format!("PC read failed: {}\n", e));
        }
    }

    match crate::commands::gdb_session::gdb_send_command("-data-list-register-values x") {
        Ok(raw) => {
            report.push_str("Registers:\n");
            for line in raw.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("^done") {
                    if let Some(values) = crate::commands::gdb_session::extract_mi_list(trimmed, "register-values") {
                        let mut reg_strs = Vec::new();
                        for reg_str in crate::commands::gdb_session::split_mi_values(&values) {
                            let number = crate::commands::gdb_session::extract_mi_field(reg_str, "number").unwrap_or_else(|| "?".into());
                            let value = crate::commands::gdb_session::extract_mi_field(reg_str, "value").unwrap_or_else(|| "?".into());
                            reg_strs.push(format!("  r{}={}", number, value));
                        }
                        report.push_str(&reg_strs.join("\n"));
                        report.push('\n');
                    }
                }
            }
        }
        Err(e) => {
            report.push_str(&format!("Registers read failed: {}\n", e));
        }
    }

    crate::commands::gdb_session::disconnect_session_sync();

    Ok(report)
}

fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs.to_string()
}

fn is_protected_hardware_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    name.eq_ignore_ascii_case("hardware_pins.h")
}

fn is_protected_toolchain_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let path_str = path.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".cfg") && path_str.contains("openocd") {
        return true;
    }
    if path_str.contains("esp-idf") || path_str.contains("espressif") || path_str.contains(".espressif") {
        return true;
    }
    false
}

fn is_hardware_config_json(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    name == "hardware_config.json"
        && path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some(".espsmith")
}

fn regenerate_hardware_pins(project_root: &Path) {
    let config_path = project_root
        .join(".espsmith")
        .join("hardware_config.json");
    let config: serde_json::Value = match std::fs::read_to_string(&config_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => return,
        },
        Err(_) => return,
    };

    let peripherals = match config.get("peripherals").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return,
    };

    let mut header = String::new();
    header.push_str("/**\n");
    header.push_str(" * hardware_pins.h — 项目硬件引脚配置\n");
    header.push_str(" * EspSmith 自动生成，请勿手动编辑\n");
    header.push_str(" * 修改引脚请通过硬件配置面板操作\n");
    header.push_str(" */\n\n");
    header.push_str("#ifndef HARDWARE_PINS_H\n");
    header.push_str("#define HARDWARE_PINS_H\n\n");
    header.push_str("#include <stdint.h>\n\n");

    let mut ids: Vec<&String> = peripherals.keys().collect();
    ids.sort();

    for id in &ids {
        let p = &peripherals[*id];
        let safe_name = id.to_uppercase().replace('-', "_");
        let def_id = p
            .get("definition_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or(id);
        let library = p
            .get("library_choice")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let notes = p.get("notes").and_then(|v| v.as_str()).unwrap_or("");

        header.push_str(&format!("/* ── {} ({}) ──────── */\n", name, def_id));
        header.push_str(&format!("/* 驱动库: {} */\n", library));
        if !notes.is_empty() {
            header.push_str(&format!("/* 备注: {} */\n", notes));
        }

        if let Some(pins) = p.get("pin_values").and_then(|v| v.as_object()) {
            let mut pin_names: Vec<&String> = pins.keys().collect();
            pin_names.sort();
            for pin_name in pin_names {
                if let Some(pin_value) = pins[pin_name].as_u64() {
                    let macro_name =
                        format!("{}_{}", safe_name, pin_name.to_uppercase());
                    header.push_str(&format!(
                        "#define {:<40} {}\n",
                        macro_name, pin_value
                    ));
                }
            }
        }

        header.push('\n');
    }

    header.push_str("#endif // HARDWARE_PINS_H\n");

    let main_dir = project_root.join("main");
    let _ = std::fs::create_dir_all(&main_dir);
    let _ = std::fs::write(main_dir.join("hardware_pins.h"), header);
}
