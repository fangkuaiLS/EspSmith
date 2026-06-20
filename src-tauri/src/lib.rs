//! EspSmith 后端入口
//!
//! 模块化命令结构：
//! - commands/project.rs    - 项目管理
//! - commands/filesystem.rs - 文件系统
//! - commands/hardware.rs   - 硬件配置
//! - commands/build.rs      - 编译/烧录
//! - commands/serial.rs     - 串口通信
//! - commands/debug.rs      - GDB 调试
//! - commands/git_cmd.rs   - Git 集成
//! - mcp.rs                 - MCP Server
//! - idf.rs                 - ESP-IDF 工具封装
//! - ai_assistant.rs        - codewhale 集成

mod commands;
mod connection;
mod mcp;
mod idf;
mod sdkconfig;
mod sdkconfig_loader;
mod ai_assistant;
mod ai_provider;
mod self_healing;
mod instruments;
mod experience;
mod adapters;
mod confserver;

use std::io::Write;
use std::sync::Mutex;

/// Commands that need the global lock (long-running operations that must not
/// run concurrently — build, flash, set-target, closed-loop, etc.)
const LOCKED_COMMANDS: &[&str] = &[
    "build", "flash", "monitor", "build-flash-monitor",
    "closed-loop", "jtag-runtime-check",
];

/// RAII global command lock: prevents concurrent espsmith.exe long-running commands.
/// Creates `%TEMP%/espsmith/command.lock` with the current PID and command name.
/// If a lock already exists and the PID is still alive, returns an error.
/// The lock is automatically removed on drop.
struct GlobalCommandLock {
    lock_path: std::path::PathBuf,
}

impl GlobalCommandLock {
    pub fn acquire(command: &str) -> Result<Self, String> {
        let lock_dir = std::env::temp_dir().join("espsmith");
        let lock_path = lock_dir.join("command.lock");
        let my_pid = std::process::id();

        // Check for existing lock
        if lock_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&lock_path) {
                // Format: "PID COMMAND"
                let mut parts = content.trim().splitn(2, ' ');
                if let (Some(pid_str), Some(old_cmd)) = (parts.next(), parts.next()) {
                    if let Ok(old_pid) = pid_str.parse::<u32>() {
                        // Re-entrant: same process already holds the lock (e.g. run_cli → cmd_closed_loop → call_tool)
                        if old_pid == my_pid {
                            // Return a no-op lock that won't delete the file on drop
                            return Ok(Self { lock_path: std::path::PathBuf::new() });
                        }
                        if is_pid_alive(old_pid) {
                            return Err(format!(
                                "Another espsmith command is running: '{}' (PID {}). Please wait for it to finish.",
                                old_cmd, old_pid
                            ));
                        }
                    }
                }
            }
            // Stale lock — remove it
            let _ = std::fs::remove_file(&lock_path);
        }

        // Create lock dir if needed
        let _ = std::fs::create_dir_all(&lock_dir);

        // Write current PID and command name
        let content = format!("{} {}", my_pid, command);
        std::fs::write(&lock_path, content)
            .map_err(|e| format!("Failed to create command lock: {}", e))?;

        Ok(Self { lock_path })
    }
}

impl Drop for GlobalCommandLock {
    fn drop(&mut self) {
        // Only remove lock file if it's a real path (not the re-entrant no-op)
        if !self.lock_path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.lock_path);
        }
    }
}

/// Check if a process with the given PID is still alive (Windows)
#[cfg(target_os = "windows")]
fn is_pid_alive(pid: u32) -> bool {
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // CREATE_NO_WINDOW — avoid console flash when checking PID
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x08000000);
    cmd.output()
        .map(|o| {
            let out = String::from_utf8_lossy(&o.stdout);
            out.contains(&pid.to_string())
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn is_pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Global state holding the confserver process while SDK config is open.
pub struct SdkConfigState(pub Mutex<Option<confserver::ConfserverProcess>>);

use commands::*;
use tauri::{Emitter, Manager};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// 初始化日志系统
fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "esp_smith=info,tauri=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 捕获 panic 信息并记录到日志，避免崩溃时丢失诊断信息。
    // 注意：原生崩溃（栈溢出、访问违规等）不会被 panic hook 捕获，
    // 但 Rust panic（unwrap 失败、数组越界等）会被捕获。
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!("PANIC: {}\nBacktrace:\n{}", info, backtrace);
        eprintln!("PANIC: {}\nBacktrace:\n{}", info, backtrace);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

    tauri::Builder::default()
        .setup(|app| {
            app.manage(SdkConfigState(Mutex::new(None)));
            connection::start_port_watcher(app.handle().clone());
            // 初始化内嵌的 CodeWhale 二进制路径
            if let Ok(resource_dir) = app.path().resource_dir() {
                ai_assistant::init_bundled_codewhale(&resource_dir);
            }
            // 初始化全局 AppHandle，供 sink 在非 Tauri 命令上下文中 emit 事件
            ai_assistant::init_app_handle(app.handle().clone());
            // 启动 IPC 服务器：让 espsmith-cli.exe 子进程能把 RunnerEvent 实时传回主进程
            self_healing::ipc::start_ipc_server();
            // 注册委托处理器：CLI 子进程通过 IPC 委托主进程执行 Self-Healing 引擎（实时进度）
            self_healing::ipc::register_delegate_handler(Box::new(|command, args| {
                run_delegate_command(command, args)
            }));
            // 注册自动更新插件（GitHub Releases + ghproxy CDN 加速）
            #[cfg(desktop)]
            let _ = app.handle().plugin(tauri_plugin_updater::Builder::new().build());
            Ok(())
        })
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            // 项目管理命令
            project::open_project,
            project::get_project_info,
            project::create_project,
            project::create_project_from_template,
            project::save_project_config,
            project::load_project_config,
            // 文件系统命令
            filesystem::read_file,
            filesystem::write_file,
            filesystem::list_directory,
            filesystem::create_file,
            filesystem::create_folder,
            filesystem::rename_file,
            filesystem::delete_file,
            filesystem::duplicate_file,
            filesystem::search_in_files,
            // 硬件配置命令
            hardware::get_hw_config,
            hardware::save_hw_config,
            hardware::check_pin_conflict,
            hardware::export_c_header,
            hardware::generate_hardware_header,
            hardware::hw_config_get_next_id,
            hardware::hw_config_add_peripheral,
            hardware::hw_config_update_peripheral,
            hardware::hw_config_remove_peripheral,
            hardware::hw_config_to_prompt,
            // 编译/烧录命令
            build::build_project,
            build::write_and_build,
            build::flash_project,
            build::get_build_errors,
            // 串口命令
            serial::list_ports,
            serial::list_ports_with_idf,
            serial::open_serial_port,
            serial::close_serial_port,
            serial::write_serial,
            // 调试命令（旧版 batch 模式，向后兼容）
            debug::get_debug_state,
            debug::set_breakpoint,
            debug::continue_execution,
            debug::step_over,
            debug::step_into,
            debug::step_out,
            debug::read_variable,
            debug::analyze_coredump,
            // 调试命令（新版 GDB 会话持久化）
            gdb_session::debug_start,
            gdb_session::debug_stop,
            gdb_session::debug_get_state,
            gdb_session::debug_set_breakpoint,
            gdb_session::debug_delete_breakpoint,
            gdb_session::debug_list_breakpoints,
            gdb_session::debug_continue,
            gdb_session::debug_step_over,
            gdb_session::debug_step_into,
            gdb_session::debug_step_out,
            gdb_session::debug_read_variable,
            gdb_session::debug_set_variable,
            gdb_session::debug_get_registers,
            gdb_session::debug_get_backtrace,
            gdb_session::debug_get_disassembly,
            gdb_session::debug_get_pc,
            gdb_session::debug_send_raw,
            gdb_session::debug_is_active,
            // OpenOCD 命令
            openocd::openocd_start,
            openocd::openocd_stop,
            openocd::openocd_is_running,
            openocd::openocd_get_chip,
            // 连接模式检测命令
            connection::detect_connection,
            connection::get_connection_mode,
            connection::force_refresh_connection,
            // Git 命令
            git_cmd::get_status,
            git_cmd::start_ai_session,
            git_cmd::commit_ai_changes,
            git_cmd::revert_ai_changes,
            // ESP-IDF 命令
            idf::idf_detect,
            idf::idf_validate_path,
            idf::idf_get_eim_setups,
            idf::idf_get_supported_targets,
            idf::idf_build,
            idf::idf_flash,
            idf::idf_monitor,
            idf::idf_set_target,
            idf::idf_menuconfig,
            idf::idf_clean,
            idf::idf_fullclean,
            idf::idf_size,
            idf::idf_size_json,
            idf::idf_erase_flash,
            idf::idf_build_flash_monitor,
            idf::idf_doctor,
            idf::idf_list_templates,
            idf::idf_read_partition_table,
            idf::idf_component_list,
            idf::idf_component_add,
            idf::idf_get_sdkconfig,
            // SDK Config via confserver
            sdkconfig::sdkconfig_load,
            sdkconfig::sdkconfig_set_value,
            sdkconfig::sdkconfig_save,
            sdkconfig::sdkconfig_close,
            idf::idf_add_arduino,
            idf::idf_efuse_summary,
            idf::idf_efuse_burn,
            idf::idf_find_tests,
            idf::idf_app_trace_start,
            idf::validate_python_path,
            // AI 助手命令
            ai_assistant::ai_start,
            ai_assistant::ai_stop,
            ai_assistant::ai_clear_session,
            ai_assistant::ai_set_session_id,
            ai_assistant::ai_send_message,
            ai_assistant::ai_get_status,
            ai_assistant::ai_get_usage,
            ai_assistant::ai_reset_usage,
            ai_assistant::ai_set_project_path,
            ai_assistant::ai_set_idf_path,
            ai_assistant::ai_set_target_chip,
            ai_assistant::ai_get_target_chip,
            ai_assistant::ai_notify_chip_changed,
            ai_assistant::ai_set_flash_port,
            ai_assistant::ai_get_flash_port,
            ai_assistant::ai_set_permission_mode,
            ai_assistant::ai_get_permission_mode,
            ai_assistant::ai_respond_permission,
            ai_assistant::check_codewhale_status,
            ai_assistant::check_mimo_status,
            ai_assistant::check_ai_backend_status,
            ai_assistant::setup_codewhale,
            // Experience 经验库管理
            ai_assistant::experience_open_dir,
            ai_assistant::experience_export,
            ai_assistant::experience_import,
            ai_assistant::experience_stats,
            // MCP 工具调用（嵌入式 MCP Server）
            mcp_call_tool,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub fn run_mcp_stdio() -> Result<(), String> {
    mcp::run_stdio_server()
}

#[tauri::command]
fn mcp_call_tool(
    app_handle: tauri::AppHandle,
    project_root: String,
    idf_path: Option<String>,
    tool_name: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // 预设置 ACTIVE_JTAG_OPERATION（如果工具类型匹配且尚未设置）
    // 基于当前 flash_port 检测连接模式，确保多设备场景下判断正确
    let is_jtag = {
        let flash_port = crate::ai_assistant::get_cached_flash_port();
        crate::connection::detect_connection_mode(flash_port.as_deref()).mode.is_jtag()
    };
    if let Some(op) = crate::ai_assistant::detect_jtag_operation_for_delegate(&tool_name, is_jtag) {
        if crate::ai_assistant::try_set_active_operation(op) {
            let active = crate::ai_assistant::lock_active_operation();
            if let Some(ref a) = *active {
                let _ = app_handle.emit("ai-operation-progress", a);
            }
        }
    }

    // sink 直接调用 handle_runner_event_for_progress 更新进度，
    // 同时 emit ai-runner-event 供前端直接使用
    let ah = app_handle.clone();
    let sink: std::sync::Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync> =
        std::sync::Arc::new(move |event: &crate::self_healing::types::RunnerEvent| {
            // 主要路径：直接更新 ACTIVE_JTAG_OPERATION 并 emit ai-operation-progress
            crate::ai_assistant::handle_runner_event_for_progress(event);
            // 辅助：emit ai-runner-event 供前端直接使用
            let _ = ah.emit("ai-runner-event", event);
        });
    let result = mcp::call_tool_direct_with_progress(
        project_root,
        idf_path,
        &tool_name,
        &args,
        Some(sink),
    );
    if result.success {
        Ok(result.data.unwrap_or_else(|| serde_json::json!({})))
    } else {
        Err(result.error.unwrap_or_else(|| "MCP tool call failed".to_string()))
    }
}

// ── CLI 子命令处理（AI 通过 exec_shell 调用）──────────────────────────────

/// 运行 CLI 命令模式，输出 JSON 结果到 stdout
pub fn run_cli() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    // 找到第一个非 flag 的参数作为命令名
    let cmd = args.iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(|a| a.as_str());

    // Acquire global command lock for long-running commands.
    // This ensures only one espsmith.exe build/flash/etc. runs at a time.
    // NOTE: For commands that support IPC delegation (closed-loop, jtag-runtime-check),
    // the lock is acquired by the delegate handler in the main process, so the CLI
    // must NOT acquire it here — otherwise the main process sees a stale lock from
    // the CLI's PID and refuses to run. We skip the lock here and let the delegate
    // path handle it; if delegation fails and we fall back to local execution,
    // the individual cmd_* functions will acquire the lock themselves.
    let _lock = match cmd {
        Some(c) if LOCKED_COMMANDS.contains(&c) => {
            // closed-loop and jtag-runtime-check use IPC delegation — skip lock here
            if matches!(c, "closed-loop" | "jtag-runtime-check") {
                None
            } else {
                Some(GlobalCommandLock::acquire(c)?)
            }
        }
        _ => None,
    };

    let result: Result<serde_json::Value, String> = match cmd {
        Some("build") => cmd_build(&args),
        Some("flash") => cmd_flash(&args),
        Some("monitor") => cmd_monitor(&args),
        Some("list-ports") => cmd_list_ports(&args),
        Some("build-flash-monitor") => cmd_build_flash_monitor(&args),
        Some("get-targets") => cmd_get_targets(&args),
        Some("doctor") => cmd_doctor(&args),
        Some("disconnect") => cmd_disconnect(),
        Some("closed-loop") => cmd_closed_loop(&args),
        Some("jtag-runtime-check") => cmd_jtag_runtime_check(&args),
        Some("openocd-start") => cmd_openocd_start(&args),
        Some("openocd-stop") => cmd_openocd_stop(&args),
        Some("openocd-is-running") => cmd_openocd_is_running(&args),
        Some("detect-connection") => cmd_detect_connection(&args),
        Some("get-connection-mode") => cmd_get_connection_mode(&args),
        Some("get-hardware-config") => cmd_get_hardware_config(&args),
        Some("get-idf-path") => cmd_get_idf_path(&args),
        Some(other) => Err(format!("Unknown command: {other}. Available: build, flash, monitor, list-ports, build-flash-monitor, get-targets, disconnect, closed-loop, jtag-runtime-check, openocd-start, openocd-stop, openocd-is-running, detect-connection, get-connection-mode, get-hardware-config, get-idf-path")),
        None => Err("No command specified. Usage: espsmith.exe <build|flash|monitor|list-ports|build-flash-monitor|get-targets|disconnect|closed-loop|jtag-runtime-check|openocd-start|openocd-stop|openocd-is-running|detect-connection|get-connection-mode|get-hardware-config|get-idf-path> [--options]".into()),
    };

    match result {
        Ok(val) => {
            println!("{}", serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string()));
            let _ = std::io::stdout().flush();
            Ok(())
        }
        Err(err) => {
            let output = serde_json::json!({"success": false, "error": err});
            println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|_| format!("{{\"success\":false,\"error\":\"{}\"}}", err)));
            let _ = std::io::stdout().flush();
            Err(err)
        }
    }
}

fn parse_named_arg(args: &[String], name: &str) -> Option<String> {
    let target = format!("--{}", name);
    let mut i = 0;
    while i < args.len() {
        if args[i] == target && i + 1 < args.len() {
            let raw = &args[i + 1];
            let trimmed = raw.trim();
            let stripped = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
                || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
            {
                trimmed[1..trimmed.len()-1].to_string()
            } else {
                trimmed.to_string()
            };
            return Some(stripped);
        }
        i += 1;
    }
    None
}

fn cmd_build(args: &[String]) -> Result<serde_json::Value, String> {
    let project = parse_named_arg(args, "project")
        .or_else(|| parse_named_arg(args, "p"))
        .ok_or("--project is required")?;
    let idf = parse_named_arg(args, "idf")
        .or_else(|| parse_named_arg(args, "i"))
        .ok_or("--idf is required")?;
    let target = parse_named_arg(args, "target");

    // If --target is specified, run idf.py set-target first to switch chip
    if let Some(ref t) = target {
        eprintln!("[espsmith] Setting target to {}", t);
        let set_result = crate::idf::run_idf_command_live(&project, &idf, &["set-target", t]);
        if let Err(e) = &set_result {
            let truncated = tail_str(e, 3000);
            return Ok(serde_json::json!({
                "success": false,
                "output": truncated,
                "errors": [],
                "error_count": 0,
                "message": format!("Failed to set target to {t}: idf.py set-target returned error")
            }));
        }
    }

    let result = crate::idf::run_idf_command_live(&project, &idf, &["build"]);
    let (success, output) = match &result {
        Ok(o) => (true, o.clone()),
        Err(o) => (false, o.clone()),
    };
    let errors = crate::idf::parse_compile_errors(&output);
    // Truncate output to avoid overwhelming the LLM with huge build logs.
    // Structured errors are preserved in the errors array regardless of truncation.
    let truncated = tail_str(&output, 3000);
    let mut resp = serde_json::json!({
        "success": success,
        "output": truncated,
        "errors": errors,
        "error_count": errors.len()
    });
    if let Some(ref t) = target {
        resp["target"] = serde_json::json!(t);
    }
    Ok(resp)
}

fn cmd_flash(args: &[String]) -> Result<serde_json::Value, String> {
    serial::disconnect_serial_sync();
    let project = parse_named_arg(args, "project")
        .or_else(|| parse_named_arg(args, "p"))
        .ok_or("--project is required")?;
    let idf = parse_named_arg(args, "idf")
        .or_else(|| parse_named_arg(args, "i"))
        .ok_or("--idf is required")?;
    let port = parse_named_arg(args, "port").ok_or("--port is required for flash")?;
    let baud: u32 = parse_named_arg(args, "baud")
        .or_else(|| parse_named_arg(args, "baudrate"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(460800);

    let baud_str = baud.to_string();
    // Use -b for esptool baud rate. Lower baud (e.g. 115200) often
    // fixes "Write timeout" on ESP32-S3 USB Serial/JTAG.
    let result = crate::idf::run_idf_command_live(&project, &idf, &["-p", &port, "-b", &baud_str, "flash"]);
    let (success, output) = match &result {
        Ok(o) => (true, o.clone()),
        Err(o) => (false, o.clone()),
    };
    Ok(serde_json::json!({
        "success": success,
        "baud": baud,
        "port": &port,
        "output": output
    }))
}

fn cmd_monitor(args: &[String]) -> Result<serde_json::Value, String> {
    serial::disconnect_serial_sync();
    let port = parse_named_arg(args, "port").ok_or("--port is required")?;
    let baudrate: u32 = parse_named_arg(args, "baudrate")
        .and_then(|v| v.parse().ok())
        .unwrap_or(115200);
    let duration: u64 = parse_named_arg(args, "duration")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);

    match crate::commands::serial::read_serial_data(&port, baudrate, duration) {
        Ok(data) => Ok(serde_json::json!({
            "success": true,
            "data": data,
            "length": data.len()
        })),
        Err(e) => Ok(serde_json::json!({
            "success": false,
            "error": e
        })),
    }
}

fn cmd_list_ports(args: &[String]) -> Result<serde_json::Value, String> {
    let idf = parse_named_arg(args, "idf");
    let ports = serialport::available_ports()
        .map(|list| {
            list.into_iter()
                .map(|p| {
                    let (vid, pid) = match &p.port_type {
                        serialport::SerialPortType::UsbPort(info) => (
                            Some(format!("{:04X}", info.vid)),
                            Some(format!("{:04X}", info.pid)),
                        ),
                        _ => (None, None),
                    };
                    serde_json::json!({
                        "name": p.port_name,
                        "port_name": p.port_name,
                        "vid": vid,
                        "pid": pid
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // 如果指定了 --idf，则对每个端口运行 chip_id 检测（参考官方扩展 processPorts）
    if let Some(ref idf_path) = idf {
        let ports_with_chip: Vec<serde_json::Value> = ports
            .into_iter()
            .map(|mut p| {
                if let Some(port_name) = p["port_name"].as_str() {
                    if let Some(chip) = crate::commands::serial::detect_chip_type_cli(idf_path, port_name) {
                        p["chip_type"] = serde_json::json!(chip);
                    }
                }
                p
            })
            .collect();
        return Ok(serde_json::json!({
            "success": true,
            "ports": ports_with_chip,
            "count": ports_with_chip.len()
        }));
    }

    Ok(serde_json::json!({
        "success": true,
        "ports": ports,
        "count": ports.len()
    }))
}

fn cmd_get_targets(args: &[String]) -> Result<serde_json::Value, String> {
    let idf = parse_named_arg(args, "idf")
        .or_else(|| parse_named_arg(args, "i"))
        .ok_or("--idf is required for get-targets")?;
    let targets = crate::idf::parse_supported_targets(&idf);
    Ok(serde_json::json!({
        "success": true,
        "idf_path": &idf,
        "targets": targets,
        "count": targets.len()
    }))
}

fn cmd_doctor(args: &[String]) -> Result<serde_json::Value, String> {
    let idf = parse_named_arg(args, "idf");
    let project = parse_named_arg(args, "project");
    crate::idf::doctor_internal(project, idf)
}

fn cmd_disconnect() -> Result<serde_json::Value, String> {
    serial::disconnect_serial_sync();
    // 等待 GUI 进程处理信号文件
    std::thread::sleep(std::time::Duration::from_millis(500));
    Ok(serde_json::json!({
        "success": true,
        "message": "Disconnect signal sent. Serial port will be released shortly."
    }))
}

fn cmd_build_flash_monitor(args: &[String]) -> Result<serde_json::Value, String> {
    let project = parse_named_arg(args, "project")
        .or_else(|| parse_named_arg(args, "p"))
        .ok_or("--project is required")?;
    let idf = parse_named_arg(args, "idf")
        .or_else(|| parse_named_arg(args, "i"))
        .ok_or("--idf is required")?;
    let port = parse_named_arg(args, "port").ok_or("--port is required")?;
    let baudrate: u32 = parse_named_arg(args, "baudrate")
        .and_then(|v| v.parse().ok())
        .unwrap_or(115200);
    let duration: u64 = parse_named_arg(args, "duration")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);
    let target = parse_named_arg(args, "target");

    // If --target is specified, run idf.py set-target first
    if let Some(ref t) = target {
        eprintln!("[espsmith] Setting target to {}", t);
        let set_result = crate::idf::run_idf_command_live(&project, &idf, &["set-target", t]);
        if let Err(e) = &set_result {
            let truncated = tail_str(e, 3000);
            return Ok(serde_json::json!({
                "success": false,
                "stage": "set-target",
                "output": truncated,
                "message": format!("Failed to set target to {t}")
            }));
        }
    }

    // 1. Build (live)
    eprintln!("[espsmith] === Build ===");
    let build = crate::idf::run_idf_command_live(&project, &idf, &["build"]);
    let (build_ok, build_out) = match &build {
        Ok(o) => (true, o.clone()),
        Err(o) => (false, o.clone()),
    };
    if !build_ok {
        return Ok(serde_json::json!({
            "success": false,
            "stage": "build",
            "build_output": build_out,
            "build_errors": crate::idf::parse_compile_errors(&build_out)
        }));
    }

    // 2. Flash (live)
    serial::disconnect_serial_sync();
    eprintln!("[espsmith] === Flash ({} @ {}baud) ===", port, baudrate);
    let baud_str = baudrate.to_string();
    let flash = crate::idf::run_idf_command_live(&project, &idf, &["-p", &port, "-b", &baud_str, "flash"]);
    let (flash_ok, flash_out) = match &flash {
        Ok(o) => (true, o.clone()),
        Err(o) => (false, o.clone()),
    };
    if !flash_ok {
        return Ok(serde_json::json!({
            "success": false,
            "stage": "flash",
            "flash_output": flash_out
        }));
    }

    // 3. Monitor
    eprintln!("[espsmith] === Serial Monitor ({}) === ", port);
    let serial = crate::commands::serial::read_serial_data(&port, baudrate, duration);
    Ok(serde_json::json!({
        "success": true,
        "target": target,
        "serial_data": serial.ok(),
        "serial_port": port
    }))
}

fn tail_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("...(truncated {} chars)\n{}", s.len() - max_chars, &s[s.len() - max_chars..])
    }
}

/// 委托处理器：在主进程中执行 Self-Healing 引擎，RunnerEvent 实时到达前端。
///
/// 核心设计：sink 直接调用 handle_runner_event_for_progress 更新进度并 emit，
/// 不再通过 broadcast_event → 全局监听器这条间接路径，避免时序竞争导致事件丢失。
fn run_delegate_command(command: &str, args: &serde_json::Value) -> self_healing::ipc::DelegateResult {
    let project = args["project"].as_str().unwrap_or("").to_string();
    let idf = args["idf"].as_str().map(|s| s.to_string());

    let tool_name = match command {
        "closed_loop" => "closed_loop",
        "jtag_runtime_check" => "jtag_runtime_check",
        _ => {
            return self_healing::ipc::DelegateResult {
                success: false,
                data: serde_json::json!({}),
                error: Some(format!("Unknown delegate command: {}", command)),
            };
        }
    };

    // 在执行引擎前先设置 ACTIVE_JTAG_OPERATION（如果尚未设置）
    // 解决时序竞争：IPC 线程中 RunnerEvent 可能在 CodeWhale 的 tool_use 事件
    // 被 tokio 任务处理之前就开始产生。
    // 使用和 MCPServer::closed_loop 相同的 is_jtag 判断逻辑，确保步骤数量匹配
    let is_jtag = {
        let conn_info = crate::connection::get_cached_connection_info();
        if conn_info.mode.is_jtag() {
            true
        } else if conn_info.mode != crate::connection::ConnectionMode::Unknown {
            false
        } else {
            // Unknown 模式下重新检测，使用 CLI 传递的端口
            let port = args["port"].as_str();
            crate::connection::detect_connection_mode(port).mode.is_jtag()
        }
    };
    tracing::info!("[Delegate] command={}, is_jtag={}, cached_mode={:?}", command, is_jtag, crate::connection::get_cached_connection_info().mode);
    if let Some(op) = crate::ai_assistant::detect_jtag_operation_for_delegate(command, is_jtag) {
        tracing::info!("[Delegate] Detected operation: type={}, steps={}", op.operation_type, op.steps.len());
        if crate::ai_assistant::try_set_active_operation(op) {
            tracing::info!("[Delegate] Pre-set ACTIVE_JTAG_OPERATION for command={}", command);
            // 立即 emit 初始进度（所有步骤 pending），让前端尽早显示进度卡片
            if let Some(ah) = crate::ai_assistant::app_handle() {
                let active = crate::ai_assistant::lock_active_operation();
                if let Some(ref a) = *active {
                    let _ = ah.emit("ai-operation-progress", a);
                }
            }
        }
    }

    // sink 直接调用 handle_runner_event_for_progress，这是进度追踪的主要路径。
    // 不再通过 broadcast_event → 全局监听器间接路径。
    let sink: std::sync::Arc<dyn Fn(&self_healing::types::RunnerEvent) + Send + Sync> =
        std::sync::Arc::new(|event: &self_healing::types::RunnerEvent| {
            crate::ai_assistant::handle_runner_event_for_progress(event);
        });

    let result = mcp::call_tool_direct_with_progress(
        project, idf, tool_name, args, Some(sink),
    );

    self_healing::ipc::DelegateResult {
        success: result.success,
        data: result.data.unwrap_or(serde_json::json!({})),
        error: result.error,
    }
}

fn cmd_closed_loop(args: &[String]) -> Result<serde_json::Value, String> {
    let project = parse_named_arg(args, "project")
        .or_else(|| parse_named_arg(args, "p"))
        .ok_or("--project is required")?;
    let idf = parse_named_arg(args, "idf")
        .or_else(|| parse_named_arg(args, "i"))
        .ok_or("--idf is required")?;
    let port = parse_named_arg(args, "port").ok_or("--port is required")?;

    let mut tool_args = serde_json::json!({ "port": port, "project": project, "idf": idf });

    if let Some(v) = parse_named_arg(args, "target") {
        tool_args["board"] = serde_json::json!(v);
    }
    if let Some(v) = parse_named_arg(args, "baudrate") {
        if let Ok(n) = v.parse::<u32>() {
            tool_args["baudrate"] = serde_json::json!(n);
        }
    }
    if let Some(v) = parse_named_arg(args, "monitor-ms") {
        if let Ok(n) = v.parse::<u64>() {
            tool_args["monitor_ms"] = serde_json::json!(n);
        }
    }
    if let Some(v) = parse_named_arg(args, "expected-pattern") {
        tool_args["expected_pattern"] = serde_json::json!(v);
    }
    if let Some(v) = parse_named_arg(args, "elf-path") {
        tool_args["elf_path"] = serde_json::json!(v);
    }
    if let Some(v) = parse_named_arg(args, "expected-pc-mask") {
        tool_args["expected_pc_mask"] = serde_json::json!(v);
    }
    if args.iter().any(|a| a == "--force-jtag") {
        tool_args["force_jtag"] = serde_json::json!(true);
    }
    if args.iter().any(|a| a == "--force-uart") {
        tool_args["force_uart"] = serde_json::json!(true);
    }

    // --ipc-addr 允许显式传递主进程 IPC 地址，避免 CodeWhale exec_shell 不传递环境变量
    let ipc_addr = parse_named_arg(args, "ipc-addr");

    // 优先委托主进程执行（RunnerEvent 实时广播到前端），回退到本地执行
    if let Some(result) = self_healing::ipc::send_delegate_and_wait_with_addr("closed_loop", &tool_args, ipc_addr.as_deref()) {
        if result.success {
            Ok(result.data)
        } else {
            Err(result.error.unwrap_or_else(|| "closed_loop failed".into()))
        }
    } else {
        tracing::warn!("[CLI] IPC delegate unavailable, running closed_loop locally");
        let sink: std::sync::Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync> =
            std::sync::Arc::new(|event: &crate::self_healing::types::RunnerEvent| {
                crate::self_healing::broadcast_event(event);
                crate::self_healing::ipc::send_event_to_parent(event);
            });
        let result = mcp::call_tool_direct_with_progress(
            project, Some(idf), "closed_loop", &tool_args, Some(sink),
        );
        if result.success {
            Ok(result.data.unwrap_or_else(|| serde_json::json!({})))
        } else {
            Err(result.error.unwrap_or_else(|| "closed_loop failed".into()))
        }
    }
}

fn cmd_jtag_runtime_check(args: &[String]) -> Result<serde_json::Value, String> {
    let project = parse_named_arg(args, "project")
        .or_else(|| parse_named_arg(args, "p"))
        .ok_or("--project is required")?;
    let idf = parse_named_arg(args, "idf")
        .or_else(|| parse_named_arg(args, "i"))
        .ok_or("--idf is required")?;
    let port = parse_named_arg(args, "port").ok_or("--port is required")?;
    let chip = parse_named_arg(args, "chip").ok_or("--chip is required")?;

    let mut tool_args = serde_json::json!({
        "port": port,
        "chip": chip,
        "project": project,
        "idf": idf,
    });

    if let Some(v) = parse_named_arg(args, "baudrate") {
        if let Ok(n) = v.parse::<u32>() {
            tool_args["baudrate"] = serde_json::json!(n);
        }
    }
    if let Some(v) = parse_named_arg(args, "monitor-ms") {
        if let Ok(n) = v.parse::<u64>() {
            tool_args["monitor_ms"] = serde_json::json!(n);
        }
    }
    if let Some(v) = parse_named_arg(args, "expected-pattern") {
        tool_args["expected_pattern"] = serde_json::json!(v);
    }
    if let Some(v) = parse_named_arg(args, "elf-path") {
        tool_args["elf_path"] = serde_json::json!(v);
    }
    if let Some(v) = parse_named_arg(args, "breakpoints") {
        let bps: Vec<serde_json::Value> = v.split(',').map(|s| serde_json::json!(s.trim())).collect();
        tool_args["breakpoints"] = serde_json::json!(bps);
    }
    if let Some(v) = parse_named_arg(args, "watch-variables") {
        let vars: Vec<serde_json::Value> = v.split(',').map(|s| serde_json::json!(s.trim())).collect();
        tool_args["watch_variables"] = serde_json::json!(vars);
    }

    // --ipc-addr 允许显式传递主进程 IPC 地址
    let ipc_addr = parse_named_arg(args, "ipc-addr");

    // 优先委托主进程执行（RunnerEvent 实时广播到前端），回退到本地执行
    if let Some(result) = self_healing::ipc::send_delegate_and_wait_with_addr("jtag_runtime_check", &tool_args, ipc_addr.as_deref()) {
        if result.success {
            Ok(result.data)
        } else {
            Err(result.error.unwrap_or_else(|| "jtag_runtime_check failed".into()))
        }
    } else {
        tracing::warn!("[CLI] IPC delegate unavailable, running jtag_runtime_check locally");
        let sink: std::sync::Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync> =
            std::sync::Arc::new(|event: &crate::self_healing::types::RunnerEvent| {
                crate::self_healing::broadcast_event(event);
                crate::self_healing::ipc::send_event_to_parent(event);
            });
        let result = mcp::call_tool_direct_with_progress(
            project, Some(idf), "jtag_runtime_check", &tool_args, Some(sink),
        );
        if result.success {
            Ok(result.data.unwrap_or_else(|| serde_json::json!({})))
        } else {
            Err(result.error.unwrap_or_else(|| "jtag_runtime_check failed".into()))
        }
    }
}

fn cmd_openocd_start(args: &[String]) -> Result<serde_json::Value, String> {
    let chip = parse_named_arg(args, "chip")
        .or_else(|| {
            let flash_port = crate::ai_assistant::get_cached_flash_port();
            crate::connection::detect_connection_mode(flash_port.as_deref())
                .chip_hint.as_ref().and_then(|h| {
                    let lower = h.to_ascii_lowercase().replace('-', "");
                    if lower == "esp32" { None } else { Some(lower) }
                })
        })
        .unwrap_or_else(|| "esp32".to_string());
    // --speed 参数：降低 JTAG 时钟频率可避免 ESP32-S3 USB-JTAG IN buffer overflow
    let speed_khz = parse_named_arg(args, "speed")
        .and_then(|s| s.parse::<u32>().ok());
    commands::openocd::ensure_openocd_running(&chip, speed_khz)?;
    Ok(serde_json::json!({
        "success": true,
        "started": true,
        "chip": chip,
        "speed_khz": speed_khz,
        "message": format!("OpenOCD started for {} (speed: {:?})", chip, speed_khz)
    }))
}

fn cmd_openocd_stop(_args: &[String]) -> Result<serde_json::Value, String> {
    commands::openocd::kill_openocd_sync();
    Ok(serde_json::json!({
        "success": true,
        "stopped": true,
        "message": "OpenOCD stopped"
    }))
}

fn cmd_openocd_is_running(_args: &[String]) -> Result<serde_json::Value, String> {
    let running = commands::openocd::is_openocd_running_sync();
    Ok(serde_json::json!({
        "success": true,
        "running": running
    }))
}

fn cmd_detect_connection(args: &[String]) -> Result<serde_json::Value, String> {
    let port = parse_named_arg(args, "port");
    let info = connection::detect_connection_mode(port.as_deref());
    serde_json::to_value(&info).map_err(|e| e.to_string())
}

fn cmd_get_connection_mode(_args: &[String]) -> Result<serde_json::Value, String> {
    let info = connection::get_cached_connection_info();
    serde_json::to_value(&info).map_err(|e| e.to_string())
}

fn cmd_get_hardware_config(args: &[String]) -> Result<serde_json::Value, String> {
    let project = parse_named_arg(args, "project")
        .or_else(|| parse_named_arg(args, "p"))
        .ok_or("--project is required")?;

    let tool_args = serde_json::json!({});
    let result = mcp::call_tool_direct(project, None, "get_hardware_config", &tool_args);
    if result.success {
        Ok(result.data.unwrap_or_else(|| serde_json::json!({})))
    } else {
        Err(result.error.unwrap_or_else(|| "get_hardware_config failed".into()))
    }
}

fn cmd_get_idf_path(args: &[String]) -> Result<serde_json::Value, String> {
    let project = parse_named_arg(args, "project")
        .or_else(|| parse_named_arg(args, "p"));

    if let Some(project) = project {
        let config_path = std::path::Path::new(&project)
            .join(".espsmith")
            .join("project.json");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("Failed to read project config: {}", e))?;
            let config: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse project config: {}", e))?;
            if let Some(idf_path) = config.get("idf_path").and_then(|v| v.as_str()) {
                return Ok(serde_json::json!({ "idf_path": idf_path }));
            }
        }
    }

    if let Ok(idf_path) = std::env::var("IDF_PATH") {
        Ok(serde_json::json!({ "idf_path": idf_path }))
    } else {
        Err("IDF_PATH not found. Set IDF_PATH environment variable or use --project to specify a project.".into())
    }
}
