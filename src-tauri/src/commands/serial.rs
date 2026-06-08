//! 串口通信命令模块
//!
//! 功能：
//! - 列出可用串口（支持 chip_id 芯片类型检测，参考官方 vscode-esp-idf-extension）
//! - 打开/关闭串口（通过 serialport crate）
//! - 读取/写入串口数据
//! - 事件推送到前端

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Mutex;
use tauri::Emitter;
use tracing::{debug, info};

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

lazy_static::lazy_static! {
    static ref ACTIVE_PORT: Mutex<Option<Box<dyn serialport::SerialPort>>> = Mutex::new(None);
}

fn disconnect_signal_path() -> std::path::PathBuf {
    std::env::temp_dir().join("espsmith-disconnect.signal")
}

fn check_disconnect_signal() -> bool {
    let path = disconnect_signal_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
        let mut guard = ACTIVE_PORT.lock().unwrap();
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

    let mut guard = ACTIVE_PORT.lock().unwrap();
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
fn read_serial_loop(app: tauri::AppHandle, port_name: String) {
    let mut buf = [0u8; 1024];
    let mut consecutive_errors: u32 = 0;
    const MAX_CONSECUTIVE_ERRORS: u32 = 100;

    loop {
        if check_disconnect_signal() {
            tracing::info!("Disconnect signal received, closing serial port");
            let _ = app.emit("serial-disconnected", serde_json::json!({
                "port": port_name,
                "reason": "disconnect_requested"
            }));
            break;
        }

        let mut guard = ACTIVE_PORT.lock().unwrap();
        if guard.is_none() {
            break;
        }

        let serial = guard.as_mut().unwrap();
        match serial.read(&mut buf) {
            Ok(n) if n > 0 => {
                consecutive_errors = 0;
                let data = String::from_utf8_lossy(&buf[..n]).to_string();
                let _ = app.emit("serial-data", serde_json::json!({
                    "port": &port_name,
                    "data": data,
                }));
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
    let mut guard = ACTIVE_PORT.lock().unwrap();
    *guard = None;
}

/// 写入串口数据
#[tauri::command]
pub async fn write_serial(data: String) -> Result<(), String> {
    info!("Writing to serial: {} bytes", data.len());
    let mut guard = ACTIVE_PORT.lock().unwrap();
    if let Some(port) = guard.as_mut() {
        port.write(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 通过 DTR/RTS 信号复位 ESP32 芯片（供 Self-Healing recovery 调用）
///
/// 复位序列：
/// 1. RTS=High → EN 拉低（芯片进入复位）
/// 2. 等待 100ms
/// 3. RTS=Low → EN 拉高（芯片退出复位）
/// 4. 等待 50ms 让芯片启动
pub fn serial_reset_via_dtr_rts() -> Result<String, String> {
    let mut guard = ACTIVE_PORT.lock().unwrap();
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

    let stream = TcpStream::connect("localhost:4444")
        .map_err(|e| format!("Cannot connect to OpenOCD telnet (localhost:4444): {}", e))?;
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
                if output.len() > 4096 {
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