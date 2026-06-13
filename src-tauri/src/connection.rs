//! 连接模式检测 — USB-JTAG vs UART 自动识别
//!
//! 通过 USB 描述符识别开发板连接方式：
//! - USB-JTAG (VID=0x303A): ESP32-S3/C3/C6/H2 内置 USB 串口/JTAG 控制器
//! - 外部 JTAG 探头: J-Link / DAP-Link / FT2232H 等
//! - UART (CP210x/CH340/FTDI): 仅串口，无 JTAG 功能

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::Emitter;
use tracing::{info, warn};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

static WATCHER_RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionMode {
    Jtag,
    Uart,
    Unknown,
}

impl ConnectionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionMode::Jtag => "jtag",
            ConnectionMode::Uart => "uart",
            ConnectionMode::Unknown => "unknown",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ConnectionMode::Jtag => "JTAG (USB-JTAG)",
            ConnectionMode::Uart => "UART (Serial)",
            ConnectionMode::Unknown => "Undetected",
        }
    }

    pub fn is_jtag(&self) -> bool {
        matches!(self, ConnectionMode::Jtag)
    }

    pub fn recommended(&self) -> bool {
        matches!(self, ConnectionMode::Jtag)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub mode: ConnectionMode,
    #[serde(rename = "modeLabel")]
    pub mode_label: String,
    pub recommended: bool,
    pub port: Option<String>,
    pub vid: Option<String>,
    pub pid: Option<String>,
    pub chip_hint: Option<String>,
    /// IDF 目标名（如 "esp32s3"、"esp32c3"），用于自动选择芯片。
    /// USB-JTAG PID 0x1001 覆盖多款芯片，此时为 None，需通过 esptool 检测。
    pub idf_target: Option<String>,
    pub capabilities: Vec<String>,
    pub recommendation: String,
}

lazy_static::lazy_static! {
    static ref CONNECTION_MODE: Mutex<ConnectionInfo> = Mutex::new(ConnectionInfo {
        mode: ConnectionMode::Unknown,
        mode_label: ConnectionMode::Unknown.label().to_string(),
        recommended: false,
        port: None,
        vid: None,
        pid: None,
        chip_hint: None,
        idf_target: None,
        capabilities: vec![],
        recommendation: String::new(),
    });
}

const ESPRESSIF_USB_VID: u16 = 0x303A;

const USB_JTAG_PID_MAP: &[(&str, u16)] = &[
    ("ESP32-S2",   0x1000),
    ("ESP32-USB-JTAG", 0x1001),  // S3/C3/C6/H2 共用此 PID
    ("ESP32-P4",   0x1007),
];

/// 外部 JTAG/调试探头的 VID 列表
const EXTERNAL_JTAG_VID_PID: &[(&str, u16, &[u16])] = &[
    // (名称, VID, 允许的PID列表 / 空表示该VID下所有PID)
    ("J-Link",             0x1366, &[]),           // SEGGER J-Link — 所有 PID 均视为 JTAG
    ("DAP-Link",           0x0D28, &[]),           // ARM DAP-Link (CMSIS-DAP)
    ("ST-Link",            0x0483, &[0x3748, 0x374B, 0x374E, 0x374F]), // ST-Link V2/V3
    ("FTDI JTAG",          0x0403, &[0x6010, 0x6011, 0x6014,             // FT2232H 系列
                                      0x8A88, 0x8A89, 0x8A8A, 0x8A8B]), // FT4232H 系列
];

#[allow(dead_code)] // 预留：PID映射表
const ESPRESSIF_SERIAL_PID_MAP: &[(&str, u16)] = &[
    ("ESP32-S2", 0x1000),
    ("ESP32-USB-JTAG", 0x1001),
];

fn chip_from_pid(pid: u16) -> Option<&'static str> {
    for (name, id) in USB_JTAG_PID_MAP {
        if *id == pid {
            return Some(name);
        }
    }
    None
}

/// 将 USB VID/PID 映射为 IDF 目标名（与 esptool chip_id 输出格式一致）。
/// PID 0x1001 覆盖 ESP32-S3/C3/C6/H2 等多款芯片，无法仅凭 PID 确定，返回 None。
fn idf_target_from_vid_pid(vid: u16, pid: u16) -> Option<&'static str> {
    if vid != ESPRESSIF_USB_VID {
        return None;
    }
    match pid {
        0x1000 => Some("ESP32-S2"),  // ESP32-S2
        0x1001 => None,              // ESP32-USB-JTAG: S3/C3/C6/H2 共用，需 esptool 检测
        0x1007 => Some("ESP32-P4"),  // ESP32-P4
        _ => None,
    }
}

fn is_espressif_usb_device(vid: u16) -> bool {
    vid == ESPRESSIF_USB_VID
}

fn is_cp210x(vid: u16, pid: u16) -> bool {
    vid == 0x10C4 && (pid == 0xEA60 || pid == 0xEA70)
}

fn is_ch340(vid: u16) -> bool {
    vid == 0x1A86
}

/// 检查是否为 FTDI **UART 芯片**（非 JTAG 探头）
fn is_ftdi_uart(vid: u16, pid: u16) -> bool {
    if vid != 0x0403 { return false; }
    // 这些 PID 是纯 UART 芯片 (FT232RL/FT232R 等)
    matches!(pid, 0x6001 | 0x6018 | 0x6019 | 0x601A | 0x601B | 0x601C | 0x601D)
}

/// 检查是否为外部 JTAG/调试探头
fn is_external_jtag_probe(vid: u16, pid: u16) -> Option<&'static str> {
    for &(name, probe_vid, pids) in EXTERNAL_JTAG_VID_PID {
        if vid == probe_vid {
            if pids.is_empty() || pids.contains(&pid) {
                return Some(name);
            }
        }
    }
    None
}

pub fn detect_connection_mode(target_port: Option<&str>) -> ConnectionInfo {
    let ports = serialport::available_ports().unwrap_or_default();

    // ── 第一优先级：精确匹配用户选择的端口 ──
    // 当指定了 target_port 时，先定位该端口并识别其模式，
    // 确保多设备场景下始终以用户选择的端口为准。
    if let Some(tp) = target_port {
        if let Some(info) = identify_single_port(&ports, tp) {
            return cache_and_log(info);
        }
        // 目标端口未找到（可能已断开），回退到全端口扫描
        warn!("Target port {} not found in available ports, falling back to scan-all", tp);
    }

    // ── 回退：扫描所有端口，选择最佳匹配（无 target_port 或目标端口不存在时） ──
    scan_best_match(&ports)
}

/// 识别单个指定端口的连接模式（用于 target_port 精确匹配）。
fn identify_single_port(ports: &[serialport::SerialPortInfo], target: &str) -> Option<ConnectionInfo> {
    let port_info = ports.iter().find(|p| p.port_name == target)?;

    if let serialport::SerialPortType::UsbPort(ref usb_info) = port_info.port_type {
        let vid = usb_info.vid;
        let pid = usb_info.pid;

        // Espressif 内置 USB-JTAG
        if is_espressif_usb_device(vid) {
            let hint = chip_from_pid(pid);
            let target_idf = idf_target_from_vid_pid(vid, pid);
            let (chip_hint_label, capabilities, recommendation) = if let Some(name) = hint {
                (
                    Some(name.to_string()),
                    vec!["flash".into(), "debug".into(), "breakpoints".into(),
                         "watchpoints".into(), "registers".into(), "backtrace".into(), "coredump".into()],
                    format!("{} 通过 USB-JTAG 连接 — 推荐使用 JTAG 模式（支持硬件断点、变量监视、调用栈分析）。", name),
                )
            } else {
                (
                    Some("ESP32-USB-JTAG".to_string()),
                    vec!["flash".into(), "debug".into(), "breakpoints".into(),
                         "watchpoints".into(), "registers".into(), "backtrace".into(), "coredump".into()],
                    "ESP32-S3/C3/C6/H2 通过 USB-JTAG 连接 — 推荐使用 JTAG 模式。".to_string(),
                )
            };
            return Some(make_info(
                ConnectionMode::Jtag,
                target, vid, pid,
                chip_hint_label, target_idf.map(|s| s.to_string()),
                capabilities, recommendation,
            ));
        }

        // 外部 JTAG 探头
        if let Some(probe_name) = is_external_jtag_probe(vid, pid) {
            return Some(make_info(
                ConnectionMode::Jtag,
                target, vid, pid,
                Some(probe_name.to_string()), None,
                vec!["flash".into(), "debug".into(), "breakpoints".into(),
                     "watchpoints".into(), "registers".into(), "backtrace".into()],
                format!("检测到外部 JTAG 探头: {}。请确保 OpenOCD 已配置且支持此探头。", probe_name),
            ));
        }

        // 纯 UART 串口
        if is_cp210x(vid, pid) || is_ch340(vid) || is_ftdi_uart(vid, pid) {
            return Some(make_info(
                ConnectionMode::Uart,
                target, vid, pid,
                None, None,
                vec!["flash".into(), "monitor".into()],
                "串口连接 (UART 模式)。如需 JTAG 调试功能，请使用内置 USB-JTAG 的开发板或连接外部 JTAG 探头。".to_string(),
            ));
        }
    }

    // USB 信息不可识别（如 PCI/Bluetooth 等非标准串口）
    None
}

/// 扫描所有端口选择最佳匹配（无 target_port 或目标端口不存在时的回退策略）。
fn scan_best_match(ports: &[serialport::SerialPortInfo]) -> ConnectionInfo {
    let mut best_mode = ConnectionMode::Unknown;
    let mut best_port: Option<String> = None;
    let mut best_vid: Option<String> = None;
    let mut best_pid: Option<String> = None;
    let mut chip_hint: Option<&str> = None;
    let mut idf_target: Option<&str> = None;
    let mut capabilities: Vec<String> = Vec::new();
    let mut recommendation = String::new();

    for port_info in ports {
        if let serialport::SerialPortType::UsbPort(ref usb_info) = port_info.port_type {
            let vid = usb_info.vid;
            let pid = usb_info.pid;

            // ── 优先级 1: Espressif 内置 USB-JTAG (VID=0x303A) ──
            if is_espressif_usb_device(vid) {
                if best_mode == ConnectionMode::Unknown {
                    let hint = chip_from_pid(pid);
                    let target = idf_target_from_vid_pid(vid, pid);
                    best_mode = ConnectionMode::Jtag;
                    best_port = Some(port_info.port_name.clone());
                    best_vid = Some(format!("{:04X}", vid));
                    best_pid = Some(format!("{:04X}", pid));
                    chip_hint = hint;
                    idf_target = target;
                    capabilities = vec![
                        "flash".into(), "debug".into(), "breakpoints".into(),
                        "watchpoints".into(), "registers".into(), "backtrace".into(), "coredump".into(),
                    ];
                    recommendation = format!(
                        "{} 通过 USB-JTAG 连接 — 推荐使用 JTAG 模式。",
                        hint.unwrap_or("ESP 芯片")
                    );
                }

            // ── 优先级 2: 外部 JTAG 探头 (J-Link / DAP-Link / ST-Link / FTDI JTAG) ──
            } else if let Some(probe_name) = is_external_jtag_probe(vid, pid) {
                if best_mode == ConnectionMode::Unknown {
                    best_mode = ConnectionMode::Jtag;
                    best_port = Some(port_info.port_name.clone());
                    best_vid = Some(format!("{:04X}", vid));
                    best_pid = Some(format!("{:04X}", pid));
                    chip_hint = Some(probe_name);
                    capabilities = vec![
                        "flash".into(), "debug".into(), "breakpoints".into(),
                        "watchpoints".into(), "registers".into(), "backtrace".into(),
                    ];
                    recommendation = format!(
                        "检测到外部 JTAG 探头: {}。请确保 OpenOCD 已配置且支持此探头，并在项目设置中选择正确的芯片型号。",
                        probe_name
                    );
                }

            // ── 优先级 3: 纯 UART 串口 (CP210x / CH340 / FTDI UART) ──
            } else if best_mode == ConnectionMode::Unknown && (is_cp210x(vid, pid) || is_ch340(vid) || is_ftdi_uart(vid, pid)) {
                best_mode = ConnectionMode::Uart;
                best_port = Some(port_info.port_name.clone());
                best_vid = Some(format!("{:04X}", vid));
                best_pid = Some(format!("{:04X}", pid));
                capabilities = vec!["flash".into(), "monitor".into()];
                recommendation = "串口连接 (UART 模式)。如需 JTAG 调试功能，请使用内置 USB-JTAG 的开发板或连接外部 JTAG 探头。".into();
            }
        }
    }

    make_info(
        best_mode,
        best_port.as_deref().unwrap_or(""),
        best_vid.as_ref().and_then(|v| u16::from_str_radix(v, 16).ok()).unwrap_or(0),
        best_pid.as_ref().and_then(|p| u16::from_str_radix(p, 16).ok()).unwrap_or(0),
        chip_hint.map(|s| s.to_string()),
        idf_target.map(|s| s.to_string()),
        capabilities,
        recommendation,
    )
}

/// 构造 ConnectionInfo 的辅助函数。
fn make_info(
    mode: ConnectionMode,
    port: &str,
    vid: u16,
    pid: u16,
    chip_hint: Option<String>,
    idf_target: Option<String>,
    capabilities: Vec<String>,
    recommendation: String,
) -> ConnectionInfo {
    let info = ConnectionInfo {
        mode,
        mode_label: mode.label().to_string(),
        recommended: mode.recommended(),
        port: if port.is_empty() { None } else { Some(port.to_string()) },
        vid: if vid == 0 { None } else { Some(format!("{:04X}", vid)) },
        pid: if pid == 0 { None } else { Some(format!("{:04X}", pid)) },
        chip_hint,
        idf_target,
        capabilities,
        recommendation,
    };
    cache_and_log(info)
}

/// 缓存连接信息到全局状态并记录日志。
fn cache_and_log(info: ConnectionInfo) -> ConnectionInfo {
    if let Ok(mut guard) = CONNECTION_MODE.lock() {
        *guard = info.clone();
    }
    info!("Connection detected: {:?} on {:?}, idf_target={:?}", info.mode, info.port, info.idf_target);
    info
}

pub fn get_cached_connection_info() -> ConnectionInfo {
    CONNECTION_MODE
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|e| e.into_inner().clone())
}

#[allow(dead_code)] // CLI使用
pub fn get_connection_mode_sync() -> ConnectionMode {
    CONNECTION_MODE
        .lock()
        .map(|g| g.mode)
        .unwrap_or_else(|e| e.into_inner().mode)
}

#[allow(dead_code)] // CLI使用
pub fn is_jtag_mode() -> bool {
    get_connection_mode_sync().is_jtag()
}

/// 检测指定端口上的连接模式，返回带能力信息的详细结构。
/// 如果 port 为 None，则扫描所有端口并选择最佳匹配。
#[tauri::command]
pub async fn detect_connection(port: Option<String>) -> Result<ConnectionInfo, String> {
    Ok(detect_connection_mode(port.as_deref()))
}

#[tauri::command]
pub async fn get_connection_mode() -> Result<ConnectionInfo, String> {
    Ok(get_cached_connection_info())
}

#[tauri::command]
pub async fn force_refresh_connection(port: Option<String>) -> Result<ConnectionInfo, String> {
    let info = detect_connection_mode(port.as_deref());
    info!("Connection refreshed: {:?} on {:?}", info.mode, info.port);
    Ok(info)
}

fn port_fingerprint() -> String {
    let ports = serialport::available_ports().unwrap_or_default();
    let mut items: Vec<String> = ports.iter().map(|p| {
        let (vid, pid) = match &p.port_type {
            serialport::SerialPortType::UsbPort(info) => (
                format!("{:04X}", info.vid),
                format!("{:04X}", info.pid),
            ),
            _ => ("0000".into(), "0000".into()),
        };
        format!("{}:{}:{}", p.port_name, vid, pid)
    }).collect();
    items.sort();
    items.join("|")
}

pub fn start_port_watcher(app_handle: tauri::AppHandle) {
    if WATCHER_RUNNING.swap(true, Ordering::SeqCst) {
        info!("Port watcher already running");
        return;
    }

    let mut last_fingerprint = port_fingerprint();
    let mut last_mode = get_cached_connection_info().mode;

    thread::spawn(move || {
        info!("Port watcher started, interval=2s");
        while WATCHER_RUNNING.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(2));

            let current = port_fingerprint();
            if current != last_fingerprint {
                info!(
                    "Port change detected! old={:?}, new={:?}",
                    last_fingerprint.len() / 30,
                    current.len() / 30
                );
                last_fingerprint = current;

                // 使用当前用户选择的 flash_port 作为 target_port，
                // 确保连接模式检测与 UI 选择的串口一致
                let current_flash_port = crate::ai_assistant::get_cached_flash_port();
                let mut info = detect_connection_mode(current_flash_port.as_deref());
                let new_mode = info.mode;

                // If VID/PID couldn't determine the exact chip (e.g. PID 0x1001),
                // run esptool chip_id to detect it immediately instead of waiting
                // for the frontend's 2s polling cycle + esptool delay.
                if info.idf_target.is_none() && info.port.is_some() && info.mode == ConnectionMode::Jtag {
                    if let Some(ref port) = info.port {
                        if let Some(chip) = detect_chip_via_esptool(port) {
                            info!("Port watcher: esptool detected chip={} on {}", chip, port);
                            info.idf_target = Some(chip.clone());
                            // Also update chip_hint to the specific chip instead of "ESP32-USB-JTAG"
                            info.chip_hint = Some(chip);
                        }
                    }
                }

                // 端口变化时始终发送事件，以便前端自动选择芯片和串口
                // （不仅限于模式变化，新设备插入同模式也需要通知）
                let mode_changed = new_mode != last_mode;
                if mode_changed {
                    info!(
                        "Connection mode changed: {:?} → {:?} (on {:?})",
                        last_mode, new_mode, info.port
                    );
                    last_mode = new_mode;
                } else {
                    info!(
                        "Port changed (mode unchanged {:?}), new port={:?}",
                        new_mode, info.port
                    );
                }

                match serde_json::to_value(&info) {
                    Ok(payload) => {
                        if let Err(e) = app_handle.emit("connection_changed", payload) {
                            warn!("Failed to emit connection_changed event: {}", e);
                        }
                    }
                    Err(e) => warn!("Failed to serialize connection info: {}", e),
                }
            }
        }
        info!("Port watcher stopped");
    });
}

/// Use esptool.py to detect the exact chip type on the given port.
/// This is called by the port watcher when VID/PID alone can't determine the chip
/// (e.g. PID 0x1001 which is shared by ESP32-S3/C3/C6/H2).
fn detect_chip_via_esptool(port: &str) -> Option<String> {
    // Try to find IDF path from the cached connection info or project config
    let idf_path = find_idf_path_for_detection()?;

    let esptool = crate::idf::find_esptool_py(&idf_path)?;
    let python = find_python_for_esptool(&idf_path)?;

    let mut cmd = std::process::Command::new(&python);
    cmd.arg(&esptool)
       .args(["--port", port, "--chip", "auto", "chip_id"]);
    #[cfg(windows)]
    { cmd.creation_flags(0x08000000); }
    let output = cmd.output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

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

/// Find IDF path for esptool detection — reads from the AI backend's cached config
fn find_idf_path_for_detection() -> Option<String> {
    crate::ai_assistant::get_cached_idf_path()
}

/// Find Python interpreter for esptool (same logic as serial.rs)
fn find_python_for_esptool(idf_path: &str) -> Option<String> {
    if let Some(eim) = crate::idf::find_eim_setup_public(idf_path) {
        return Some(eim.python);
    }
    for cmd_name in &["python", "python3"] {
        let mut cmd = std::process::Command::new(cmd_name);
        cmd.arg("--version");
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(cmd_name.to_string());
        }
    }
    None
}

#[allow(dead_code)] // 生命周期管理预留
pub fn stop_port_watcher() {
    WATCHER_RUNNING.store(false, Ordering::SeqCst);
    info!("Port watcher stop requested");
}