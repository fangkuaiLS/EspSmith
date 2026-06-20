//! GDB 会话持久化模块
//!
//! 通过 GDB/MI (Machine Interface) 协议与 GDB 子进程保持长连接，
//! 替代旧的每次启动新 GDB 进程的 batch 模式。
//!
//! 功能：
//! - 启动/停止 GDB 会话
//! - 设置/删除/列出断点
//! - 继续执行/单步/步入/步出
//! - 读取变量值
//! - 读取寄存器
//! - 获取调用栈
//! - Core dump 分析（独立会话）

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugState {
    pub running: bool,
    pub pc: String,
    pub stack: Vec<StackFrame>,
    pub locals: Vec<(String, String)>,
    pub registers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    pub level: u32,
    pub function: String,
    pub file: String,
    pub line: u32,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: u32,
    pub file: String,
    pub line: u32,
    pub address: String,
    pub enabled: bool,
    pub hit_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    pub value: String,
    pub type_name: String,
}

#[allow(dead_code)] // 预留：GDB会话元数据
pub(crate) struct GdbSession {
    child: Child,
    stdin: std::process::ChildStdin,
    /// GDB stdout 读取泵的接收端。后台线程持续 read_line 并通过此通道投递，
    /// 使 send_command 可用 recv_timeout 实现命令级超时，避免无限阻塞。
    rx: std::sync::mpsc::Receiver<GdbLine>,
    #[allow(dead_code)] // 预留：GDB会话元数据
    target: String,
    #[allow(dead_code)] // 预留：GDB会话元数据
    gdb_binary: String,
    connected: bool,
    token: u32,
}

/// 读取泵投递的一行结果。
enum GdbLine {
    /// 一行 stdout 内容（不含尾随换行）
    Line(String),
    /// stdout 已关闭（GDB 进程退出），后续不会再有数据
    Eof,
}

impl GdbSession {
    fn send_command(&mut self, cmd: &str) -> Result<String, String> {
        self.token += 1;
        let token = self.token;
        let full_cmd = format!("{token}{cmd}\n");

        self.stdin
            .write_all(full_cmd.as_bytes())
            .map_err(|e| format!("Failed to write to GDB stdin: {e}"))?;
        self.stdin
            .flush()
            .map_err(|e| format!("Failed to flush GDB stdin: {e}"))?;

        let mut output = String::new();
        // 命令级超时：GDB 正常响应很快（<1s），15 秒无响应视为挂死。
        // 注意：-target-select 涉及远程 JTAG 连接，可能需要几秒，15s 足够。
        const CMD_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);
        let mut recv_count = 0usize;
        loop {
            let line = match self.rx.recv_timeout(CMD_DEADLINE) {
                Ok(GdbLine::Line(l)) => l,
                Ok(GdbLine::Eof) => {
                    self.connected = false;
                    return Err("GDB process terminated unexpectedly (EOF). Restart debug session.".into());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    self.connected = false;
                    return Err(format!(
                        "GDB command timed out after {}s (no '(gdb)' prompt, received {} lines). GDB may be hung; restart debug session.",
                        CMD_DEADLINE.as_secs(), recv_count
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.connected = false;
                    return Err("GDB read pump disconnected unexpectedly.".into());
                }
            };
            recv_count += 1;
            let trimmed = line.trim();
            if trimmed == "(gdb)" {
                break;
            }
            output.push_str(&line);
            output.push('\n');
            if trimmed.starts_with(&format!("{token}^error")) {
                let msg = extract_mi_field(trimmed, "msg");
                return Err(format!("GDB error: {}", msg.unwrap_or_else(|| trimmed.into())));
            }
        }
        Ok(output)
    }

    fn send_mi_and_get_result(&mut self, cmd: &str) -> Result<String, String> {
        let raw = self.send_command(cmd)?;
        let token_str = self.token.to_string();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(&format!("{token_str}^done")) {
                return Ok(trimmed.to_string());
            }
            if trimmed.starts_with(&format!("{token_str}^error")) {
                let msg = extract_mi_field(trimmed, "msg");
                return Err(format!("GDB error: {}", msg.unwrap_or_else(|| trimmed.into())));
            }
        }
        Ok(raw)
    }
}

/// 启动 GDB stdout 常驻读取泵。返回接收端。
/// 后台线程持续 read_line 并通过通道投递，GdbSession::send_command 用 recv_timeout 消费。
fn spawn_read_pump(stdout: std::process::ChildStdout) -> std::sync::mpsc::Receiver<GdbLine> {
    let (tx, rx) = std::sync::mpsc::channel::<GdbLine>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(GdbLine::Eof);
                    break;
                }
                Ok(_) => {
                    if tx.send(GdbLine::Line(std::mem::take(&mut line))).is_err() {
                        break; // 接收端已丢弃
                    }
                }
                Err(_) => {
                    let _ = tx.send(GdbLine::Eof);
                    break;
                }
            }
        }
    });
    rx
}

lazy_static::lazy_static! {
    pub(crate) static ref GDB_SESSION: Mutex<Option<GdbSession>> = Mutex::new(None);
}

pub(crate) fn gdb_send_command(cmd: &str) -> Result<String, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("No active GDB session")?;
    check_session_alive(session)?;
    session.send_command(cmd)
}

pub(crate) fn gdb_send_mi(cmd: &str) -> Result<String, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("No active GDB session")?;
    check_session_alive(session)?;
    session.send_mi_and_get_result(cmd)
}

fn check_session_alive(session: &mut GdbSession) -> Result<(), String> {
    if !session.connected {
        return Ok(());
    }
    match session.child.try_wait() {
        Ok(Some(status)) => {
            session.connected = false;
            Err(format!("GDB process has exited (status={}). Restart debug session.", status))
        }
        Ok(None) => Ok(()),
        Err(e) => {
            session.connected = false;
            Err(format!("GDB process check failed: {}. Restart debug session.", e))
        }
    }
}

pub(crate) fn extract_mi_field(record: &str, field: &str) -> Option<String> {
    let pattern = format!("{}=\"", field);
    if let Some(start) = record.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = record[value_start..].find('"') {
            let raw = &record[value_start..value_start + end];
            return Some(unescape_mi_string(raw));
        }
    }
    None
}

fn unescape_mi_string(s: &str) -> String {
    s.replace("\\\"", "\"")
        .replace("\\\\", "\\")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
}

pub(crate) fn resolve_gdb_binary(target_chip: Option<&str>) -> String {
    match target_chip.map(|c| c.to_ascii_lowercase()).as_deref() {
        Some("esp32c3") | Some("esp32c5") | Some("esp32c6") | Some("esp32c61")
        | Some("esp32h2") | Some("esp32p4") => "riscv32-esp-elf-gdb".into(),
        Some("esp32s2") => "xtensa-esp32s2-elf-gdb".into(),
        Some("esp32s3") => "xtensa-esp32s3-elf-gdb".into(),
        Some("esp32") | Some(_) | None => "xtensa-esp32-elf-gdb".into(),
    }
}

pub(crate) fn find_gdb_binary(target_chip: Option<&str>) -> Result<String, String> {
    let gdb_name = resolve_gdb_binary(target_chip);

    if let Ok(path) = std::env::var("ESP_GDB_BIN") {
        let p = std::path::Path::new(&path);
        if p.exists() {
            tracing::info!("GDB binary from ESP_GDB_BIN: {}", path);
            return Ok(path);
        }
        tracing::warn!("ESP_GDB_BIN is set to '{}' but file does not exist", path);
    }

    let home = dirs_next::home_dir().unwrap_or_default();
    let espressif_tools = home.join(".espressif").join("tools");
    if let Ok(found) = search_gdb_in_dir(&espressif_tools, &gdb_name) {
        return Ok(found);
    }

    let alt_tools = [
        std::path::PathBuf::from("C:\\Espressif\\tools"),
        home.join(".espressif").join("dist"),
    ];
    for tools_dir in &alt_tools {
        if tools_dir.exists() {
            if let Ok(found) = search_gdb_in_dir(tools_dir, &gdb_name) {
                return Ok(found);
            }
        }
    }

    if let Ok(path) = std::env::var("IDF_TOOLS_PATH") {
        let tools = std::path::Path::new(&path);
        if let Ok(found) = search_gdb_in_dir(tools, &gdb_name) {
            return Ok(found);
        }
    }

    if let Ok(idf) = std::env::var("IDF_PATH") {
        let tools_from_idf = std::path::Path::new(&idf)
            .parent()
            .map(|p| p.join(".espressif").join("tools"))
            .unwrap_or_default();
        if tools_from_idf.exists() {
            if let Ok(found) = search_gdb_in_dir(&tools_from_idf, &gdb_name) {
                return Ok(found);
            }
        }
    }

    if let Ok(openocd_path) = std::env::var("OPENOCD_BIN") {
        let openocd_dir = std::path::Path::new(&openocd_path);
        if let Some(tools_root) = openocd_dir
            .parent()
            .and_then(|p| p.parent())
            .or_else(|| openocd_dir.parent())
        {
            if tools_root.exists() {
                if let Ok(found) = search_gdb_in_dir(tools_root, &gdb_name) {
                    tracing::info!("GDB binary inferred from OPENOCD_BIN: {}", found);
                    return Ok(found);
                }
            }
        }
    }

    if let Ok(path) = which::which(&gdb_name) {
        tracing::info!("GDB binary from PATH: {}", path.display());
        return Ok(path.to_string_lossy().to_string());
    }

    Err(format!(
        "GDB binary '{}' not found.\n\
         Install options:\n\
         1. Set ESP_GDB_BIN environment variable: $env:ESP_GDB_BIN='C:\\path\\to\\{gdb_name}.exe'\n\
         2. Install via ESP-IDF tools installer\n\
         3. Download from https://github.com/espressif/binutils-gdb/releases\n\
         4. Add to PATH",
        gdb_name
    ))
}

fn search_gdb_in_dir(tools_root: &std::path::Path, gdb_name: &str) -> Result<String, String> {
    if !tools_root.exists() {
        return Err("tools root does not exist".into());
    }
    let mut found: Option<String> = None;
    walk_dir_for_gdb(tools_root, gdb_name, 4, &mut found);
    match found {
        Some(path) => Ok(path),
        None => Err("not found".into()),
    }
}

fn walk_dir_for_gdb(dir: &std::path::Path, gdb_name: &str, max_depth: u32, found: &mut Option<String>) {
    if max_depth == 0 || found.is_some() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "bin") {
                let candidate = path.join(gdb_name);
                if candidate.exists() {
                    let path_str = candidate.to_string_lossy().to_string();
                    tracing::info!("GDB binary found: {}", path_str);
                    *found = Some(path_str);
                    return;
                }
            }
            walk_dir_for_gdb(&path, gdb_name, max_depth - 1, found);
            if found.is_some() {
                return;
            }
        }
    }
}

#[allow(dead_code)] // 预留：GDB输出截断
fn tail_lines(output: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

#[tauri::command]
pub async fn debug_start(
    elf_path: Option<String>,
    target: Option<String>,
    target_chip: Option<String>,
) -> Result<DebugState, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        info!("Debug session already active, connecting to new target");
        let _ = guard.take();
    }

    let gdb_binary = find_gdb_binary(target_chip.as_deref())?;
    let target_addr = target.unwrap_or_else(|| "localhost:3333".into());

    info!("Starting GDB session: {} -> {}", gdb_binary, target_addr);

    let mut cmd = Command::new(&gdb_binary);
    cmd.args(["--interpreter=mi2", "-nx", "-quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    { cmd.creation_flags(0x08000000); }
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to start GDB ({gdb_binary}): {e}"))?;

    let stdin = child
        .stdin
        .take()
        .ok_or("Failed to capture GDB stdin")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture GDB stdout")?;
    let rx = spawn_read_pump(stdout);

    let mut session = GdbSession {
        child,
        stdin,
        rx,
        target: target_addr.clone(),
        gdb_binary: gdb_binary.clone(),
        connected: false,
        token: 0,
    };

    session.send_command("")?;
    let elf_normalized = elf_path.as_deref().map(crate::adapters::normalize_path_for_gdb).unwrap_or_default();
    session
        .send_mi_and_get_result(&format!("-file-exec-and-symbols {}", elf_normalized))
        .map_err(|e| format!("Failed to load ELF: {e}"))?;

    session
        .send_mi_and_get_result(&format!("-target-select remote {}", target_addr))
        .map_err(|e| format!("Failed to connect to target: {e}"))?;

    session.connected = true;

    let state = collect_debug_state(&mut session)?;
    *guard = Some(session);
    Ok(state)
}

#[tauri::command]
pub async fn debug_stop() -> Result<(), String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    if let Some(mut session) = guard.take() {
        info!("Stopping GDB session");
        let _ = session.send_command("-gdb-exit");
        let _ = session.child.kill();
        let _ = session.child.wait();
    }
    Ok(())
}

#[tauri::command]
pub async fn debug_get_state() -> Result<DebugState, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;
    if !session.connected {
        return Ok(DebugState {
            running: false,
            pc: "".into(),
            stack: vec![],
            locals: vec![],
            registers: vec![],
        });
    }
    collect_debug_state(session)
}

#[tauri::command]
pub async fn debug_set_breakpoint(file: String, line: u32) -> Result<Breakpoint, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Setting breakpoint at {}:{}", file, line);
    let resp = session.send_mi_and_get_result(&format!("-break-insert -f {}:{}", file, line))?;

    let id = extract_mi_field(&resp, "number")
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let addr = extract_mi_field(&resp, "addr").unwrap_or_else(|| "?".into());

    Ok(Breakpoint {
        id,
        file,
        line,
        address: addr,
        enabled: true,
        hit_count: 0,
    })
}

#[tauri::command]
pub async fn debug_delete_breakpoint(id: u32) -> Result<(), String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Deleting breakpoint {}", id);
    session.send_mi_and_get_result(&format!("-break-delete {}", id))?;
    Ok(())
}

#[tauri::command]
pub async fn debug_list_breakpoints() -> Result<Vec<Breakpoint>, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    let raw = session.send_command("-break-list")?;
    let mut bps = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(body) = trimmed.strip_prefix("^done") {
            if let Some(table) = extract_mi_list(body, "BreakpointTable") {
                if let Some(body_list) = extract_mi_list(&table, "body") {
                    for bp_str in split_mi_values(&body_list) {
                        let id = extract_mi_field(bp_str, "number")
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0);
                        let enabled = extract_mi_field(bp_str, "enabled")
                            .map(|e| e == "y")
                            .unwrap_or(false);
                        let file = extract_mi_field(bp_str, "fullname")
                            .or_else(|| extract_mi_field(bp_str, "file"))
                            .unwrap_or_else(|| "?".into());
                        let line_num = extract_mi_field(bp_str, "line")
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0);
                        let addr = extract_mi_field(bp_str, "addr")
                            .unwrap_or_else(|| "?".into());
                        let hit = extract_mi_field(bp_str, "times")
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0);
                        bps.push(Breakpoint {
                            id,
                            file,
                            line: line_num,
                            address: addr,
                            enabled,
                            hit_count: hit,
                        });
                    }
                }
            }
        }
    }
    Ok(bps)
}

#[tauri::command]
pub async fn debug_continue() -> Result<DebugState, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Continuing execution");
    let _ = session.send_mi_and_get_result("-exec-continue --all")?;
    collect_debug_state(session)
}

#[tauri::command]
pub async fn debug_step_over() -> Result<DebugState, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Step over");
    let _ = session.send_mi_and_get_result("-exec-next")?;
    collect_debug_state(session)
}

#[tauri::command]
pub async fn debug_step_into() -> Result<DebugState, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Step into");
    let _ = session.send_mi_and_get_result("-exec-step")?;
    collect_debug_state(session)
}

#[tauri::command]
pub async fn debug_step_out() -> Result<DebugState, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Step out");
    let _ = session.send_mi_and_get_result("-exec-finish")?;
    collect_debug_state(session)
}

#[tauri::command]
pub async fn debug_read_variable(name: String) -> Result<VariableInfo, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Reading variable: {}", name);
    let resp = session.send_mi_and_get_result(&format!("-var-create - * {}", name))?;

    let var_name = extract_mi_field(&resp, "name").unwrap_or_else(|| name.clone());
    let value = extract_mi_field(&resp, "value").unwrap_or_else(|| "?".into());
    let type_name = extract_mi_field(&resp, "type").unwrap_or_else(|| "?".into());

    let _ = session.send_mi_and_get_result(&format!("-var-delete {}", var_name));

    Ok(VariableInfo {
        name,
        value,
        type_name,
    })
}

#[tauri::command]
pub async fn debug_set_variable(name: String, value: String) -> Result<(), String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    info!("Setting variable {} = {}", name, value);
    session.send_mi_and_get_result(&format!("-gdb-set var {} = {}", name, value))?;
    Ok(())
}

#[tauri::command]
pub async fn debug_get_registers() -> Result<Vec<(String, String)>, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    let raw = session.send_command("-data-list-register-values x")?;
    let mut regs = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("^done") {
            if let Some(values) = extract_mi_list(trimmed, "register-values") {
                for reg_str in split_mi_values(&values) {
                    let number = extract_mi_field(reg_str, "number")
                        .unwrap_or_else(|| "?".into());
                    let value = extract_mi_field(reg_str, "value")
                        .unwrap_or_else(|| "?".into());
                    regs.push((format!("r{}", number), value));
                }
            }
        }
    }
    Ok(regs)
}

#[tauri::command]
pub async fn debug_get_backtrace() -> Result<Vec<StackFrame>, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    let raw = session.send_command("-stack-list-frames")?;
    let mut frames = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("^done") {
            if let Some(stack) = extract_mi_list(trimmed, "stack") {
                for frame_str in split_mi_values(&stack) {
                    let level = extract_mi_field(frame_str, "level")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0);
                    let function = extract_mi_field(frame_str, "func")
                        .unwrap_or_else(|| "?".into());
                    let file = extract_mi_field(frame_str, "fullname")
                        .or_else(|| extract_mi_field(frame_str, "file"))
                        .unwrap_or_else(|| "?".into());
                    let line = extract_mi_field(frame_str, "line")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0);
                    let address = extract_mi_field(frame_str, "addr")
                        .unwrap_or_else(|| "?".into());
                    frames.push(StackFrame {
                        level,
                        function,
                        file,
                        line,
                        address,
                    });
                }
            }
        }
    }
    Ok(frames)
}

#[tauri::command]
pub async fn debug_get_disassembly() -> Result<String, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    let raw = session.send_command("-data-disassemble -s \"$pc\" -e \"$pc+40\" -- 0")?;
    Ok(raw)
}

#[tauri::command]
pub async fn debug_get_pc() -> Result<String, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    let raw = session.send_mi_and_get_result("-data-evaluate-expression \"$pc\"")?;
    let value = extract_mi_field(&raw, "value").unwrap_or_else(|| "0x0".into());
    Ok(value)
}

#[tauri::command]
pub async fn debug_send_raw(command: String) -> Result<String, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard
        .as_mut()
        .ok_or("No active debug session. Use debug_start first.")?;

    let raw = session.send_command(&command)?;
    Ok(raw)
}

#[tauri::command]
pub async fn debug_is_active() -> Result<bool, String> {
    let guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    Ok(guard.is_some())
}

fn collect_debug_state(session: &mut GdbSession) -> Result<DebugState, String> {
    let pc = session
        .send_mi_and_get_result("-data-evaluate-expression \"$pc\"")
        .ok()
        .and_then(|r| extract_mi_field(&r, "value"))
        .unwrap_or_else(|| "0x0".into());

    let bt_raw = session.send_command("-stack-list-frames 0 20")?;
    let mut stack = Vec::new();
    for line in bt_raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("^done") {
            if let Some(frames) = extract_mi_list(trimmed, "stack") {
                for frame_str in split_mi_values(&frames) {
                    let level = extract_mi_field(frame_str, "level")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0);
                    let function = extract_mi_field(frame_str, "func")
                        .unwrap_or_else(|| "?".into());
                    let file = extract_mi_field(frame_str, "fullname")
                        .or_else(|| extract_mi_field(frame_str, "file"))
                        .unwrap_or_else(|| "?".into());
                    let line = extract_mi_field(frame_str, "line")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0);
                    let address = extract_mi_field(frame_str, "addr")
                        .unwrap_or_else(|| "?".into());
                    stack.push(StackFrame {
                        level,
                        function,
                        file,
                        line,
                        address,
                    });
                }
            }
        }
    }

    let regs_raw = session.send_command("-data-list-register-values x")?;
    let mut registers = Vec::new();
    for line in regs_raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("^done") {
            if let Some(values) = extract_mi_list(trimmed, "register-values") {
                for reg_str in split_mi_values(&values) {
                    let number =
                        extract_mi_field(reg_str, "number").unwrap_or_else(|| "?".into());
                    let value =
                        extract_mi_field(reg_str, "value").unwrap_or_else(|| "?".into());
                    registers.push((format!("r{}", number), value));
                }
            }
        }
    }

    let locals_raw = session.send_command("-stack-list-locals 0")?;
    let mut locals = Vec::new();
    for line in locals_raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("^done") {
            if let Some(local_vars) = extract_mi_list(trimmed, "locals") {
                for var_str in split_mi_values(&local_vars) {
                    let name = extract_mi_field(var_str, "name")
                        .unwrap_or_else(|| "?".into());
                    let value = extract_mi_field(var_str, "value")
                        .unwrap_or_else(|| "?".into());
                    locals.push((name, value));
                }
            }
        }
    }

    Ok(DebugState {
        running: session.connected,
        pc,
        stack,
        locals,
        registers,
    })
}

pub(crate) fn extract_mi_list(record: &str, key: &str) -> Option<String> {
    let search = format!("{}=[", key);
    if let Some(start) = record.find(&search) {
        let value_start = start + search.len();
        let mut depth = 1;
        let mut end = value_start;
        let bytes = record.as_bytes();
        while end < bytes.len() && depth > 0 {
            match bytes[end] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(record[value_start..end].to_string());
                    }
                }
                _ => {}
            }
            end += 1;
        }
    }
    None
}

pub(crate) fn split_mi_values(list_str: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    let bytes = list_str.as_bytes();
    for (i, &ch) in bytes.iter().enumerate() {
        match ch {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    items.push(&list_str[start..=i]);
                    start = i + 2;
                }
            }
            _ => {}
        }
    }
    items
}

#[allow(dead_code)] // 预留：MI模式coredump分析
pub async fn analyze_coredump_mi(
    elf_path: String,
    dump_path: String,
    target_chip: Option<String>,
) -> Result<String, String> {
    let gdb_binary = find_gdb_binary(target_chip.as_deref())?;

    let mut cmd = Command::new(&gdb_binary);
    cmd.args(["--interpreter=mi2", "-nx", "-quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    { cmd.creation_flags(0x08000000); }
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to start GDB for coredump: {e}"))?;

    let stdin = child
        .stdin
        .take()
        .ok_or("Failed to capture GDB stdin")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture GDB stdout")?;
    let rx = spawn_read_pump(stdout);

    let mut session = GdbSession {
        child,
        stdin,
        rx,
        target: "coredump".into(),
        gdb_binary,
        connected: false,
        token: 0,
    };

    session.send_command("")?;
    let elf_normalized = crate::adapters::normalize_path_for_gdb(&elf_path);
    let dump_normalized = crate::adapters::normalize_path_for_gdb(&dump_path);
    session
        .send_mi_and_get_result(&format!("-file-exec-and-symbols {}", elf_normalized))
        .map_err(|e| format!("Failed to load ELF: {e}"))?;
    session
        .send_mi_and_get_result(&format!("-file-exec-file {}", dump_normalized))
        .map_err(|e| format!("Failed to load core dump: {e}"))?;

    let mut report = String::new();

    report.push_str("=== Backtrace ===\n");
    match session.send_command("-stack-list-frames") {
        Ok(bt) => {
            for line in bt.lines() {
                if let Some(stripped) = line.strip_prefix('~') {
                    report.push_str(stripped.trim_matches('"'));
                    report.push('\n');
                } else if !line.starts_with('^') && !line.starts_with('&') {
                    report.push_str(line);
                    report.push('\n');
                }
            }
        }
        Err(e) => {
            report.push_str(&format!("Backtrace failed: {e}\n"));
        }
    }

    report.push_str("\n=== Registers ===\n");
    match session.send_command("-data-list-register-values x") {
        Ok(regs) => {
            report.push_str(&regs);
        }
        Err(e) => {
            report.push_str(&format!("Register read failed: {e}\n"));
        }
    }

    report.push_str("\n=== Current Frame ===\n");
    match session.send_mi_and_get_result("-stack-info-frame") {
        Ok(frame) => {
            report.push_str(&frame);
            report.push('\n');
        }
        Err(e) => {
            report.push_str(&format!("Frame info failed: {e}\n"));
        }
    }

    info!("Coredump analysis complete: {} chars", report.len());
    let _ = session.send_command("-gdb-exit");
    let _ = session.child.wait();

    Ok(report)
}

pub fn disconnect_session_sync() {
    if let Ok(mut guard) = GDB_SESSION.lock() {
        if let Some(mut session) = guard.take() {
            info!("Disconnecting GDB session (sync)");
            // 直接 kill GDB 进程：当目标程序仍在运行时，GDB 不会响应 -gdb-exit，
            // send_command 会阻塞 15s 超时。kill + wait 是最可靠的清理方式。
            let _ = session.child.kill();
            let _ = session.child.wait();
        }
    }
}

pub fn connect_session_sync(elf_path: &str, target: &str, target_chip: &str) -> Result<(), String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        let _ = guard.take();
    }

    let gdb_binary = find_gdb_binary(Some(target_chip))?;
    let gdb_name_for_display = gdb_binary.clone();

    info!("Auto-connecting GDB session: {} -> {}", gdb_binary, target);

    let mut cmd = std::process::Command::new(&gdb_binary);
    cmd.args(["--interpreter=mi2", "-nx", "-quiet"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());
    #[cfg(windows)]
    { cmd.creation_flags(0x08000000); }
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to start GDB ({gdb_name_for_display}): {e}"))?;

    let stdin = child.stdin.take().ok_or("Failed to capture GDB stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to capture GDB stdout")?;

    let rx = spawn_read_pump(stdout);

    let mut session = GdbSession {
        child,
        stdin,
        rx,
        target: target.to_string(),
        gdb_binary: gdb_binary.clone(),
        connected: false,
        token: 0,
    };

    // 读取初始化输出直到 (gdb) 提示符（5 秒超时）
    let mut init_output = String::new();
    let init_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut init_line_count = 0usize;
    while std::time::Instant::now() < init_deadline {
        match session.rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(GdbLine::Line(l)) => {
                init_line_count += 1;
                init_output.push_str(&l);
                let trimmed = l.trim();
                if trimmed == "(gdb)" || trimmed.ends_with("(gdb)") {
                    break;
                }
            }
            Ok(GdbLine::Eof) => return Err("GDB process terminated during init".into()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("GDB read pump disconnected during init".into())
            }
        }
    }
    info!("GDB init output: {} bytes, {} lines", init_output.len(), init_line_count);

    let elf_path_normalized = crate::adapters::normalize_path_for_gdb(elf_path);
    session.send_mi_and_get_result(&format!("-file-exec-and-symbols {}", elf_path_normalized))
        .map_err(|e| format!("Failed to load ELF: {e}"))?;

    // ESP32-S3 dual-core SMP workaround: set hardware watchpoint limit
    // before connecting to avoid "Remote 'g' packet reply is too long" error.
    let _ = session.send_mi_and_get_result("-gdb-set remote hardware-watchpoint-limit 2");
    let _ = session.send_mi_and_get_result("-gdb-set remote hardware-breakpoint-limit 6");

    session.send_mi_and_get_result(&format!("-target-select extended-remote {}", target))
        .map_err(|e| format!("Failed to connect to target: {e}"))?;
    session.connected = true;

    *guard = Some(session);
    info!("GDB session auto-connected for {}", target_chip);
    Ok(())
}

pub fn send_mi_command_sync(cmd: &[u8]) -> Result<String, String> {
    let cmd_str = std::str::from_utf8(cmd).map_err(|e| format!("Invalid UTF-8: {}", e))?;
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("No active GDB session")?;
    check_session_alive(session)?;
    let raw = session.send_command(cmd_str)?;
    let token_str = session.token.to_string();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{token_str}^done")) {
            return Ok(trimmed.to_string());
        }
        if trimmed.starts_with(&format!("{token_str}^error")) {
            let msg = extract_mi_field(trimmed, "msg");
            return Err(format!("GDB error: {}", msg.unwrap_or_else(|| trimmed.into())));
        }
    }
    Ok(raw)
}

/// After `-exec-continue`, GDB runs the program asynchronously. When a
/// breakpoint is hit, GDB sends a `*stopped` async record followed by the
/// `(gdb)` prompt. This function reads the channel for `*stopped` within
/// the given timeout.
///
/// Returns `Ok(Some(output))` if `*stopped` was received (breakpoint hit).
/// Returns `Ok(None)` if the timeout elapsed without `*stopped` (program
/// still running — caller should skip state queries and proceed to cleanup).
/// Returns `Err` on EOF / channel disconnect.
pub fn read_async_stopped_event(timeout_ms: u64) -> Result<Option<String>, String> {
    let mut guard = GDB_SESSION.lock().map_err(|e| e.to_string())?;
    let session = guard.as_mut().ok_or("No active GDB session")?;
    check_session_alive(session)?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let mut output = String::new();
    let mut found_stopped = false;

    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(if found_stopped { Some(output) } else { None });
        }

        match session.rx.recv_timeout(remaining) {
            Ok(GdbLine::Line(l)) => {
                let trimmed = l.trim();
                output.push_str(&l);
                output.push('\n');
                if trimmed.starts_with("*stopped") {
                    found_stopped = true;
                } else if trimmed == "(gdb)" {
                    if found_stopped {
                        return Ok(Some(output));
                    }
                    // Prompt without *stopped — keep waiting for the actual stop event.
                }
            }
            Ok(GdbLine::Eof) => {
                session.connected = false;
                return Err("GDB process terminated unexpectedly (EOF).".into());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return Ok(if found_stopped { Some(output) } else { None });
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                session.connected = false;
                return Err("GDB read pump disconnected unexpectedly.".into());
            }
        }
    }
}

pub fn get_debug_state_sync() -> String {
    let mut parts: Vec<String> = vec![];

    if let Ok(pc) = gdb_send_command("-data-evaluate-expression $pc") {
        parts.push(format!("PC: {}", pc));
    }
    if let Ok(sp) = gdb_send_command("-data-evaluate-expression $sp") {
        parts.push(format!("SP: {}", sp));
    }
    if let Ok(bt) = gdb_send_command("-stack-info-depth 10") {
        parts.push(format!("Stack: {}", bt));
    }
    if let Ok(frames) = gdb_send_command("-stack-list-frames 0 10") {
        parts.push(frames);
    }
    if let Ok(bt) = gdb_send_command("-stack-list-frames 0 10") {
        parts.push(bt);
    }

    parts.join("\n")
}