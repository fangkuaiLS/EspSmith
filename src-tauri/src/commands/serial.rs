//! 串口通信命令模块
//!
//! 功能：
//! - 列出可用串口（支持 chip_id 芯片类型检测，参考官方 vscode-esp-idf-extension）
//! - 打开/关闭串口（通过 serialport crate）
//! - 读取/写入串口数据
//! - 事件推送到前端

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tracing::{debug, info, warn};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialPortInfo {
    pub name: String,
    pub path: String,
    pub vid: Option<String>,
    pub pid: Option<String>,
    /// 通过 esptool.py chip_id 检测到的芯片类型（如 "ESP32-S3"）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chip_type: Option<String>,
}

/// 环形缓冲区中单行日志条目。
/// `ts_ms` 为系统启动以来的单调毫秒数（Instant 基准），用于 since() 增量读取。
#[derive(Debug, Clone, Serialize)]
pub struct RingEntry {
    /// 自进程串口读取起点起算的单调毫秒时间戳
    pub ts_ms: u64,
    pub line: String,
}

/// 内存环形缓冲区：持续累积串口日志，满容量后自动滚动。
/// 同时供 GUI 与 AI（MCP 工具）读取，保证两者看到完全一致的数据。
pub struct RingBuffer {
    lines: VecDeque<RingEntry>,
    max_lines: usize,
    /// 读取线程启动时刻（作为 ts_ms 的零点）
    epoch: Instant,
}

impl RingBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(max_lines.min(8192)),
            max_lines,
            epoch: Instant::now(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    /// 写入一行（调用方负责行分割）。满容量时自动淘汰最旧条目。
    pub fn push_line(&mut self, line: String) -> u64 {
        let ts = self.now_ms();
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
        }
        self.lines.push_back(RingEntry { ts_ms: ts, line });
        ts
    }

    /// 取最近 n 行
    pub fn tail(&self, n: usize) -> Vec<RingEntry> {
        let n = n.min(self.lines.len());
        self.lines.iter().rev().take(n).rev().cloned().collect()
    }

    /// 取时间戳 since_ms 之后的所有行（不含等于）
    pub fn since(&self, since_ms: u64) -> Vec<RingEntry> {
        self.lines
            .iter()
            .filter(|e| e.ts_ms > since_ms)
            .cloned()
            .collect()
    }

    /// 正则搜索历史日志，返回最多 limit 条匹配
    pub fn search(&self, re: &regex::Regex, limit: usize) -> Vec<RingEntry> {
        self.lines
            .iter()
            .filter(|e| re.is_match(&e.line))
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 当前最新一行的时间戳（无数据返回 0）
    pub fn latest_ts(&self) -> u64 {
        self.lines.back().map(|e| e.ts_ms).unwrap_or(0)
    }
}

/// 待 AI 消费的崩溃现场快照。
pub struct PendingCrash {
    pub captured_at_unix: u64,
    /// 崩溃前的日志（最近 ~300 行）
    pub log_before: Vec<RingEntry>,
    /// 崩溃信息摘要（detect_crash_patterns 的命中结果）
    pub crash_summary: String,
    /// 若 JTAG 可用，附带的 GDB backtrace（可选）
    pub gdb_backtrace: Option<String>,
}

lazy_static::lazy_static! {
    static ref ACTIVE_PORT: Mutex<Option<Box<dyn serialport::SerialPort>>> = Mutex::new(None);

    /// 共享环形缓冲区：GUI 与 AI 均通过此读取串口历史
    static ref RING_BUFFER: Arc<RwLock<RingBuffer>> =
        Arc::new(RwLock::new(RingBuffer::new(configured_ring_lines())));

    /// 待消费的崩溃现场（一次性的，AI 取走后清空）
    static ref PENDING_CRASH: Arc<Mutex<Option<PendingCrash>>> =
        Arc::new(Mutex::new(None));
}

/// 环形缓冲区行容量（默认 50000，约 10MB）。
/// 可通过环境变量 ESPSMITH_RING_LINES 覆盖（最小 1000）。
fn configured_ring_lines() -> usize {
    std::env::var("ESPSMITH_RING_LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &usize| n >= 1000)
        .unwrap_or(50_000)
}

/// 崩溃捕获的上下文行数（崩溃前回溯多少行）
fn configured_crash_capture_lines() -> usize {
    std::env::var("ESPSMITH_CRASH_LINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &usize| n >= 50)
        .unwrap_or(300)
}

/// 是否在检测到崩溃时自动复位恢复（默认 true）
pub fn auto_recover_on_crash() -> bool {
    std::env::var("ESPSMITH_AUTO_RECOVER")
        .ok()
        .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "no" | "off"))
        .unwrap_or(true)
}

// ===== 公共读取 API（供 MCP 工具与 CLI 调用）=====

/// 取环形缓冲区最近 n 行
pub fn ring_tail(n: usize) -> Vec<RingEntry> {
    RING_BUFFER.read().unwrap_or_else(|e| e.into_inner()).tail(n)
}

/// 取 since_ms 之后的所有行
pub fn ring_since(since_ms: u64) -> Vec<RingEntry> {
    RING_BUFFER.read().unwrap_or_else(|e| e.into_inner()).since(since_ms)
}

/// 正则搜索历史日志
pub fn ring_search(pattern: &str, limit: usize) -> Result<Vec<RingEntry>, String> {
    let re = regex::Regex::new(pattern).map_err(|e| format!("Invalid regex: {e}"))?;
    Ok(RING_BUFFER.read().unwrap_or_else(|e| e.into_inner()).search(&re, limit))
}

/// 缓冲区当前条目数
pub fn ring_len() -> usize {
    RING_BUFFER.read().unwrap_or_else(|e| e.into_inner()).len()
}

/// `ring_wait_for_output` 的返回值，带来源标识。
/// AI 可通过 `source` 字段区分"有新数据"和"兜底返回历史日志"。
#[derive(Debug, Clone)]
pub struct WaitOutput {
    /// 串口文本内容
    pub text: String,
    /// 数据来源：
    /// - `"new_data"`: flash 后收到的新输出
    /// - `"fallback_tail"`: 无新数据，返回最近 500 行兜底（设备可能未启动）
    /// - `"empty"`: ring buffer 完全为空
    pub source: &'static str,
}

/// 智能等待并返回新输出。
///
/// 行为：
/// 1. 记录起始时间戳 ts0；
/// 2. 在最多 `max_wait_ms`（上限 30s，由调用方传入）内轮询，每 100ms 检查一次；
/// 3. 一旦累计字节数达到 `enough_bytes`，再等 300ms 收齐这批，然后返回；
/// 4. 超时仍未达阈值，返回已收到的所有新行；
/// 5. 完全无新数据时，返回最近 500 行作为兜底。
///
/// 这样既避免固定睡眠浪费时间，又能让复杂固件（WiFi/BLE 初始化）有足够启动时间。
pub fn ring_wait_for_output(max_wait_ms: u64, enough_bytes: usize) -> WaitOutput {
    let max_wait_ms = max_wait_ms.min(30_000);
    let ts0 = ring_latest_ts();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_wait_ms);

    loop {
        let now_entries = RING_BUFFER.read().unwrap_or_else(|e| e.into_inner()).since(ts0);
        let total_bytes: usize = now_entries.iter().map(|e| e.line.len() + 1).sum();

        if total_bytes >= enough_bytes {
            // 收齐这批：再等 300ms 让连续输出落定
            std::thread::sleep(std::time::Duration::from_millis(300));
            break;
        }

        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let entries = RING_BUFFER.read().unwrap_or_else(|e| e.into_inner()).since(ts0);
    if entries.is_empty() {
        // 完全无新数据：返回最近 500 行兜底
        let tail = ring_tail(500);
        if tail.is_empty() {
            WaitOutput {
                text: String::new(),
                source: "empty",
            }
        } else {
            WaitOutput {
                text: tail
                    .iter()
                    .map(|e| e.line.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                source: "fallback_tail",
            }
        }
    } else {
        WaitOutput {
            text: entries
                .iter()
                .map(|e| e.line.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            source: "new_data",
        }
    }
}

/// 最新行时间戳（无数据返回 0）
pub fn ring_latest_ts() -> u64 {
    RING_BUFFER.read().unwrap_or_else(|e| e.into_inner()).latest_ts()
}

/// 是否有串口处于连接（读取中）状态
pub fn is_serial_open() -> bool {
    ACTIVE_PORT.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// 取走待消费的崩溃现场（取走后清空，避免重复投递给 AI）
pub fn take_pending_crash() -> Option<PendingCrash> {
    PENDING_CRASH.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// 是否存在未消费的崩溃现场
pub fn has_pending_crash() -> bool {
    PENDING_CRASH.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

// ===== 崩溃模式检测（与 mcp.rs 共用）=====

/// ESP32 常见崩溃/异常模式。读线程与 closed_loop 验证共用此列表。
pub const CRASH_PATTERNS: &[&str] = &[
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

/// 检测文本中是否包含崩溃特征，命中则返回拼接的命中模式字符串。
pub fn detect_crash_patterns(text: &str) -> String {
    let mut found = Vec::new();
    for pattern in CRASH_PATTERNS {
        if text.contains(pattern) {
            found.push(*pattern);
        }
    }
    if found.is_empty() {
        return String::new();
    }
    format!("Detected crash signatures: {}", found.join(", "))
}

/// 捕获崩溃现场：截取环形缓冲尾部日志，调用方可选择附带 GDB backtrace。
/// 写入 PENDING_CRASH 并返回是否首次写入（避免同一崩溃反复覆盖）。
pub fn capture_crash_context(crash_summary: String, gdb_backtrace: Option<String>) -> bool {
    let capture_lines = configured_crash_capture_lines();
    let log_before = {
        let ring = RING_BUFFER.read().unwrap_or_else(|e| e.into_inner());
        ring.tail(capture_lines)
    };
    let captured_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let crash = PendingCrash {
        captured_at_unix,
        log_before,
        crash_summary,
        gdb_backtrace,
    };
    let mut slot = PENDING_CRASH.lock().unwrap_or_else(|e| e.into_inner());
    let was_empty = slot.is_none();
    *slot = Some(crash);
    was_empty
}

fn disconnect_signal_path() -> std::path::PathBuf {
    std::env::temp_dir().join("espsmith-disconnect.signal")
}

fn check_disconnect_signal() -> bool {
    let path = disconnect_signal_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
        let mut guard = ACTIVE_PORT.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
        true
    } else {
        false
    }
}

/// ESP32 USB-to-Serial 常见 VID/PID 列表（官方扩展类似过滤逻辑）
#[allow(dead_code)] // 预留：ESP32芯片VID识别
const ESPRESSIF_VIDS: &[u16] = &[0x303A, 0x10C4, 0x0403, 0x1A86, 0x2341, 0x2E8A];

#[allow(dead_code)] // 预留：串口设备过滤
fn is_esp_serial_device(vid: u16, pid: u16) -> bool {
    // Espressif CP210x: 10C4:EA60
    if vid == 0x303A {
        return pid == 0x1001  // USB Serial/JTAG (ESP32-S3, ESP32-C3, etc.)
            || pid == 0x4001  // USB Serial/JTAG v1
            || pid == 0x4002  // USB Serial/JTAG v2
            || pid == 0x4003  // USB Serial/JTAG v3
            || pid == 0x4004;
    }
    // CP210x family
    if vid == 0x10C4 && pid == 0xEA60 { return true; }
    // FTDI
    if vid == 0x0403 && (pid == 0x6001 || pid == 0x6010 || pid == 0x6014) { return true; }
    // CH340/CH341
    if vid == 0x1A86 && (pid == 0x7523 || pid == 0x5523) { return true; }
    // Arduino / Atmel
    if vid == 0x2341 { return true; }
    // Raspberry Pi Pico
    if vid == 0x2E8A { return true; }
    // Any VID in ESP list
    ESPRESSIF_VIDS.contains(&vid)
}

/// 使用 esptool.py 检测指定端口的芯片类型（参考官方 SerialPort.processPorts）
///
/// 执行 `python esptool.py --port <port> chip_id`，从输出中提取芯片型号
fn detect_chip_type(idf_path: &str, port: &str) -> Option<String> {
    let esptool = crate::idf::find_esptool_py(idf_path)?;
    let python = find_python_for_idf(idf_path)?;

    let mut cmd = Command::new(&python);
    cmd.arg(&esptool)
       .args(["--port", port, "--chip", "auto", "chip_id"]);
    #[cfg(windows)]
    { cmd.creation_flags(0x08000000); }
    let output = cmd.output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    // esptool.py chip_id 输出格式多种：
    // "Chip is ESP32-S3 (revision v0.2)"
    // "Detected chip type: ESP32-S3"
    // "Chip ID: 0x09 (ESP32-S3)"
    for line in combined.lines() {
        let trimmed = line.trim();
        for chip in &["ESP32-S3", "ESP32-S2", "ESP32-C3", "ESP32-C2", "ESP32-C5", "ESP32-C6", "ESP32-C61", "ESP32-H2", "ESP32-H21", "ESP32-H4", "ESP32-P4", "ESP32"] {
            if trimmed.contains(chip) {
                return Some(chip.to_string());
            }
        }
    }
    None
}

/// 查找用于执行 esptool.py 的 Python 解释器
fn find_python_for_idf(idf_path: &str) -> Option<String> {
    // 优先使用 EIM 配置的 Python
    if let Some(eim) = crate::idf::find_eim_setup_public(idf_path) {
        return Some(eim.python);
    }
    // 回退：尝试系统 Python
    for cmd_name in &["python", "python3"] {
        let mut cmd = Command::new(cmd_name);
        cmd.arg("--version");
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(cmd_name.to_string());
        }
    }
    None
}

/// 列出可用串口（Tauri 命令）
/// 支持可选的 `idf_path` 参数来启用芯片类型检测
#[tauri::command]
pub async fn list_ports_with_idf(idf_path: Option<String>) -> Result<Vec<SerialPortInfo>, String> {
    debug!("Listing serial ports (detect_chips={})", idf_path.is_some());
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;

    let mut result: Vec<SerialPortInfo> = ports.into_iter().map(|p| {
        let (vid, pid) = match &p.port_type {
            serialport::SerialPortType::UsbPort(info) => (
                Some(format!("{:04X}", info.vid)),
                Some(format!("{:04X}", info.pid)),
            ),
            _ => (None, None),
        };
        SerialPortInfo {
            name: p.port_name.clone(),
            path: p.port_name,
            vid,
            pid,
            chip_type: None,
        }
    }).collect();

    // 可选：通过 esptool.py chip_id 检测芯片类型（参考官方扩展 processPorts）
    if let Some(ref idf) = idf_path {
        for info in &mut result {
            if let Some(chip) = detect_chip_type(idf, &info.path) {
                info!("Port {} detected chip: {}", info.path, chip);
                info.chip_type = Some(chip);
            }
        }
    }

    Ok(result)
}

/// 列出可用串口（旧版兼容接口，无芯片检测）
#[tauri::command]
pub async fn list_ports() -> Result<Vec<SerialPortInfo>, String> {
    list_ports_with_idf(None).await
}

/// 使用 esptool.py 自动检测连接目标芯片的端口（参考官方 SerialPort.detectDefaultPort）
///
/// 执行 `python esptool.py --chip <target> chip_id` 扫描所有端口
#[allow(dead_code)] // 预留：自动端口检测
pub fn detect_default_port(idf_path: &str, target: &str) -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    for p in &ports {
        let port_name = &p.port_name;
        let esptool = crate::idf::find_esptool_py(idf_path)?;
        let python = find_python_for_idf(idf_path)?;
        let mut cmd = Command::new(&python);
        cmd.arg(&esptool)
           .args(["--port", port_name, "--chip", target, "chip_id"]);
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        let output = cmd.output().ok()?;
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() && combined.to_lowercase().contains(&target.to_lowercase()) {
            return Some(port_name.clone());
        }
    }
    None
}

/// 公开版 chip_id 检测（供 lib.rs CLI 调用）
pub fn detect_chip_type_cli(idf_path: &str, port: &str) -> Option<String> {
    detect_chip_type(idf_path, port)
}

/// 打开串口
#[tauri::command]
pub async fn open_serial_port(
    app: tauri::AppHandle,
    port: String,
    baudrate: u32,
) -> Result<(), String> {
    info!("Opening serial port: {} at {} baud", port, baudrate);

    let serial = serialport::new(&port, baudrate)
        .timeout(std::time::Duration::from_millis(100))
        .open()
        .map_err(|e| format!("Failed to open port: {}", e))?;

    let mut guard = ACTIVE_PORT.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(serial);

    // 启动后台读取线程
    let app_clone = app.clone();
    let port_clone = port.clone();
    std::thread::spawn(move || {
        read_serial_loop(app_clone, port_clone);
    });

    Ok(())
}

/// 检测 Windows 设备断开错误（USB 拔出时的典型错误码）
#[cfg(windows)]
fn is_device_disconnect_error(e: &std::io::Error) -> bool {
    const ERROR_GEN_FAILURE: i32 = 31;
    const ERROR_DEVICE_NOT_CONNECTED: i32 = 1167;
    const ERROR_NO_SUCH_DEVICE: i32 = 433;
    if let Some(code) = e.raw_os_error() {
        matches!(code, ERROR_GEN_FAILURE | ERROR_DEVICE_NOT_CONNECTED | ERROR_NO_SUCH_DEVICE)
    } else {
        false
    }
}

/// 后台读取串口数据并推送事件
///
/// 错误分类策略：
/// - TimedOut / WouldBlock：正常超时，继续读取
/// - BrokenPipe / DeviceNotFound / ConnectionReset：设备断开，发送事件并退出
/// - 其他错误：累计连续错误次数，超过阈值后断开
///
/// 数据流：每个字节块先按 `\n` 切分为完整行，完整行进入共享环形缓冲区
/// （供 GUI 与 AI 一致读取），同时 emit "serial-data" 给前端。每行还会
/// 扫描崩溃模式：命中即捕获现场、emit "crash-detected"，并可自动复位恢复。
fn read_serial_loop(app: tauri::AppHandle, port_name: String) {
    let mut buf = [0u8; 1024];
    let mut consecutive_errors: u32 = 0;
    const MAX_CONSECUTIVE_ERRORS: u32 = 100;
    // 半行续接缓冲：ESP32 日志可能跨多个 read() 到达
    let mut partial: String = String::new();
    // 简易崩溃去抖：上次触发崩溃捕获的时间戳，避免崩溃转储的后续行反复触发
    let mut last_crash_trigger_ts: u64 = 0;

    loop {
        if check_disconnect_signal() {
            tracing::info!("Disconnect signal received, closing serial port");
            let _ = app.emit("serial-disconnected", serde_json::json!({
                "port": port_name,
                "reason": "disconnect_requested"
            }));
            break;
        }

        let mut guard = ACTIVE_PORT.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            break;
        }

        let serial = guard.as_mut().unwrap();
        match serial.read(&mut buf) {
            Ok(n) if n > 0 => {
                consecutive_errors = 0;
                let data = String::from_utf8_lossy(&buf[..n]).to_string();

                // 1) 前端：保留原始 chunk 推送（兼容现有 useSerialMonitor）
                let _ = app.emit("serial-data", serde_json::json!({
                    "port": &port_name,
                    "data": data,
                }));

                // 2) 行分割 + 写入共享环形缓冲 + 崩溃扫描
                partial.push_str(&data);
                while let Some(idx) = partial.find('\n') {
                    let mut line = partial.split_off(idx + 1);
                    std::mem::swap(&mut line, &mut partial);
                    // `line` 现在是本行内容（含可能的 \r）
                    let trimmed = line.trim_end_matches('\r');

                    let ts_ms = RING_BUFFER.write().unwrap_or_else(|e| e.into_inner()).push_line(trimmed.to_string());

                    // 崩溃模式扫描（去抖：距上次触发超过 3 秒才再次捕获）
                    let crash = detect_crash_patterns(trimmed);
                    if !crash.is_empty() && ts_ms.saturating_sub(last_crash_trigger_ts) > 3000 {
                        last_crash_trigger_ts = ts_ms;
                        let summary = crash.clone();
                        // 捕获现场（无 GDB backtrace，读线程不阻塞连接 GDB）
                        let is_new = capture_crash_context(crash, None);
                        if is_new {
                            warn!("Crash detected on {}: {}", port_name, summary);
                            let _ = app.emit("crash-detected", serde_json::json!({
                                "port": &port_name,
                                "summary": summary,
                                "ts_ms": ts_ms,
                            }));
                        }

                        // 自动复位恢复（让崩溃转储写完后再复位）
                        if auto_recover_on_crash() {
                            let app_clone = app.clone();
                            let port_clone = port_name.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(800));
                                let _ = app_clone.emit("serial-auto-recover", serde_json::json!({
                                    "port": &port_clone,
                                }));
                                match serial_reset_via_dtr_rts() {
                                    Ok(msg) => info!("Auto-recover reset: {}", msg),
                                    Err(e) => warn!("Auto-recover reset failed: {}", e),
                                }
                            });
                        }
                    }
                }
            }
            Ok(_) => {
                // read() 返回 Ok(0) 表示超时无数据，正常继续
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock => {
                        // 正常超时，继续读取
                    }
                    std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted => {
                        tracing::warn!("Serial port {} disconnected: {}", port_name, e);
                        *guard = None;
                        let _ = app.emit("serial-disconnected", serde_json::json!({
                            "port": &port_name,
                            "error": e.to_string(),
                        }));
                        break;
                    }
                    _ => {
                        // Windows 设备断开通常返回 Other(31/1167)
                        #[cfg(windows)]
                        if is_device_disconnect_error(&e) {
                            tracing::warn!("Serial port {} device disconnected (Windows): {}", port_name, e);
                            *guard = None;
                            let _ = app.emit("serial-disconnected", serde_json::json!({
                                "port": &port_name,
                                "error": format!("Device disconnected: {}", e),
                            }));
                            break;
                        }

                        consecutive_errors += 1;
                        tracing::warn!(
                            "Serial read error on {}: {} (consecutive: {}/{})",
                            port_name, e, consecutive_errors, MAX_CONSECUTIVE_ERRORS
                        );
                        if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                            tracing::error!(
                                "Too many consecutive read errors on {}, disconnecting",
                                port_name
                            );
                            *guard = None;
                            let _ = app.emit("serial-disconnected", serde_json::json!({
                                "port": &port_name,
                                "error": format!("Too many consecutive errors: {}", e),
                            }));
                            break;
                        }
                    }
                }
            }
        }

        drop(guard);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    tracing::info!("Serial read loop exited for port: {}", port_name);
}

/// 关闭串口
#[tauri::command]
pub async fn close_serial_port() -> Result<(), String> {
    info!("Closing serial port");
    disconnect_serial_sync();
    Ok(())
}

/// 同步关闭串口（供 CLI 调用）
/// 写入信号文件通知 GUI 进程的串口读取循环退出
pub fn disconnect_serial_sync() {
    let signal_path = disconnect_signal_path();
    let _ = std::fs::write(&signal_path, "disconnect");
    // 同时清除本进程的 ACTIVE_PORT（CLI 进程中通常为 None）
    let mut guard = ACTIVE_PORT.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// 写入串口数据
#[tauri::command]
pub async fn write_serial(data: String) -> Result<(), String> {
    info!("Writing to serial: {} bytes", data.len());
    write_serial_shared(&data)?;
    Ok(())
}

/// 同步写入共享串口（供 MCP 工具与读线程复位后调用）。
/// 复用全局 ACTIVE_PORT，无需重新打开。
pub fn write_serial_shared(data: &str) -> Result<(), String> {
    let mut guard = ACTIVE_PORT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(port) = guard.as_mut() {
        port.write(data.as_bytes()).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("No active serial port".into())
    }
}

/// 通过 DTR/RTS 信号复位 ESP32 芯片（供 Self-Healing recovery 调用）
///
/// 复位序列：
/// 1. RTS=High → EN 拉低（芯片进入复位）
/// 2. 等待 100ms
/// 3. RTS=Low → EN 拉高（芯片退出复位）
/// 4. 等待 50ms 让芯片启动
pub fn serial_reset_via_dtr_rts() -> Result<String, String> {
    let mut guard = ACTIVE_PORT.lock().unwrap_or_else(|e| e.into_inner());
    let port = guard.as_mut().ok_or("No active serial port for reset")?;

    info!("Executing DTR/RTS reset sequence");

    port.write_data_terminal_ready(false)
        .map_err(|e| format!("DTR clear failed: {}", e))?;
    port.write_request_to_send(true)
        .map_err(|e| format!("RTS set failed: {}", e))?;
    std::thread::sleep(std::time::Duration::from_millis(100));

    port.write_request_to_send(false)
        .map_err(|e| format!("RTS clear failed: {}", e))?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    info!("DTR/RTS reset sequence completed");
    Ok("Serial reset via DTR/RTS toggle completed.".into())
}

/// 通过 OpenOCD telnet 执行软复位（供 Self-Healing recovery 调用）
pub fn probe_soft_reset() -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let stream = TcpStream::connect(crate::adapters::OPENOCD_ADDR)
        .map_err(|e| format!("Cannot connect to OpenOCD telnet ({}): {}", crate::adapters::OPENOCD_ADDR, e))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;

    let mut stream = stream;
    // 读取欢迎消息
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);

    // 发送 reset halt 命令
    stream.write_all(b"reset halt\n")
        .map_err(|e| format!("Failed to send reset command: {}", e))?;
    std::thread::sleep(std::time::Duration::from_millis(500));

    let n = stream.read(&mut buf)
        .map_err(|e| format!("Failed to read OpenOCD response: {}", e))?;
    let response = String::from_utf8_lossy(&buf[..n]);
    info!("OpenOCD reset response: {}", response.trim());

    Ok(format!("Probe soft reset executed. Response: {}", response.trim()))
}

/// 同步采样串口数据（CLI 模式使用）
pub fn read_serial_data(port: &str, baudrate: u32, duration_ms: u64) -> Result<String, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(duration_ms);
    let mut serial = serialport::new(port, baudrate)
        .timeout(std::time::Duration::from_millis(100))
        .open()
        .map_err(|e| format!("Cannot open serial port {}: {}", port, e))?;
    let mut output = Vec::new();
    let mut buf = [0u8; 512];
    while std::time::Instant::now() < deadline {
        match serial.read(&mut buf) {
            Ok(n) if n > 0 => {
                output.extend_from_slice(&buf[..n]);
                // 上限 64KB，防止极端情况下的内存膨胀（正常 boot 日志 < 8KB）
                if output.len() > 65536 {
                    break;
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(format!("Serial read error: {}", e)),
        }
    }
    Ok(String::from_utf8_lossy(&output).to_string())
}