//! OpenOCD 进程管理模块
//!
//! 管理 ESP 芯片 JTAG 调试的 OpenOCD 进程生命周期。
//! 支持多种芯片和调试接口的自动配置。

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tracing::{info, warn};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

lazy_static::lazy_static! {
    static ref OPENOCD_STATE: Mutex<Option<OpenOcdSession>> = Mutex::new(None);
}

struct OpenOcdSession {
    child: Child,
    chip: String,
    pid: u32,
}

fn find_openocd_binary() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("OPENOCD_BIN") {
        let p = PathBuf::from(&path);
        if p.exists() {
            info!("Found OpenOCD from OPENOCD_BIN env: {}", p.display());
            return Ok(p);
        }
        warn!("OPENOCD_BIN is set to '{}' but file does not exist", path);
    }

    if let Ok(idf_path) = std::env::var("IDF_PATH") {
        let candidate = PathBuf::from(&idf_path).join("tools").join("openocd").join("openocd.exe");
        if candidate.exists() {
            info!("Found OpenOCD from IDF_PATH: {}", candidate.display());
            return Ok(candidate);
        }
    }

    let home = dirs_next::home_dir().ok_or("Cannot determine home directory")?;
    let patterns = [
        home.join(".espressif").join("tools").join("openocd-esp32").join("bin").join("openocd.exe"),
        home.join(".espressif").join("tools").join("openocd").join("bin").join("openocd.exe"),
        PathBuf::from("C:\\Espressif\\tools\\openocd-esp32\\bin\\openocd.exe"),
    ];

    for p in &patterns {
        if p.exists() {
            info!("Found OpenOCD at default path: {}", p.display());
            return Ok(p.clone());
        }
    }

    match which::which("openocd") {
        Ok(p) => {
            info!("Found OpenOCD from PATH: {}", p.display());
            Ok(p)
        },
        Err(_) => Err(
            "OpenOCD not found. Please set the OPENOCD_BIN environment variable to the full path of openocd.exe (e.g. C:\\Espressif\\tools\\openocd-esp32\\bin\\openocd.exe). See README for JTAG setup instructions.".into()
        ),
    }
}

fn find_openocd_scripts_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("OPENOCD_SCRIPTS") {
        let p = PathBuf::from(&dir);
        if p.exists() {
            info!("Found OpenOCD scripts from OPENOCD_SCRIPTS env: {}", p.display());
            return Ok(p);
        }
        warn!("OPENOCD_SCRIPTS is set to '{}' but directory does not exist", dir);
    }

    if let Ok(bin) = find_openocd_binary() {
        if let Some(bin_dir) = bin.parent() {
            let candidate = bin_dir.join("..").join("share").join("openocd").join("scripts");
            if candidate.exists() {
                info!("Found OpenOCD scripts from binary path: {}", candidate.display());
                return Ok(candidate);
            }
        }
    }

    Err(
        "OpenOCD scripts directory not found. The scripts must be in <openocd_bin>/../share/openocd/scripts relative to the OpenOCD binary. Set OPENOCD_SCRIPTS env var to the correct path.".into()
    )
}

fn chip_config(chip: &str) -> Result<(&'static str, &'static str), String> {
    let normalized = chip.to_ascii_lowercase().replace('-', "");
    match normalized.as_str() {
        "esp32" => Ok(("target/esp32.cfg", "interface/ftdi/esp32_devkitj_v1.cfg")),
        "esp32s2" => Ok(("target/esp32s2.cfg", "interface/ftdi/esp32_devkitj_v1.cfg")),
        "esp32s3" => Ok(("target/esp32s3.cfg", "interface/esp_usb_jtag.cfg")),
        "esp32c3" => Ok(("target/esp32c3.cfg", "interface/esp_usb_jtag.cfg")),
        "esp32c5" => Ok(("target/esp32c5.cfg", "interface/esp_usb_jtag.cfg")),
        "esp32c6" => Ok(("target/esp32c6.cfg", "interface/esp_usb_jtag.cfg")),
        "esp32c61" => Ok(("target/esp32c61.cfg", "interface/esp_usb_jtag.cfg")),
        "esp32h2" => Ok(("target/esp32h2.cfg", "interface/esp_usb_jtag.cfg")),
        "esp32p4" => Ok(("target/esp32p4.cfg", "interface/esp_usb_jtag.cfg")),
        _ => Err(format!("Unknown chip '{}'. Supported: esp32, esp32s2, esp32s3, esp32c3, esp32c5, esp32c6, esp32c61, esp32h2, esp32p4", chip)),
    }
}

/// 根据外部探头名称返回对应的 OpenOCD interface 配置路径。
/// 返回 None 表示使用 chip_config 中的默认配置（内置 USB-JTAG 或 ESP DevKitJ）。
fn probe_interface_config(probe_name: &str) -> Option<&'static str> {
    match probe_name.to_ascii_lowercase().as_str() {
        "j-link" | "jlink" => Some("interface/jlink.cfg"),
        "dap-link" | "daplink" | "cmsis-dap" => Some("interface/cmsis-dap.cfg"),
        "st-link" | "stlink" => Some("interface/stlink.cfg"),
        "ftdi jtag" | "ftdi_jtag" => Some("interface/ftdi/esp32_devkitj_v1.cfg"),
        _ => None,
    }
}

fn patch_scripts_dir(scripts_dir: &Path) -> Result<PathBuf, String> {
    let common_cfg = scripts_dir.join("target").join("esp_common.cfg");
    if !common_cfg.exists() {
        info!("esp_common.cfg not found at {}, skipping patch", common_cfg.display());
        return Ok(scripts_dir.to_path_buf());
    }

    let content = std::fs::read_to_string(&common_cfg)
        .map_err(|e| format!("Cannot read {}: {}", common_cfg.display(), e))?;

    let re = regex::Regex::new(r"\s*-expected-id\s+\S+").unwrap();
    let patched = re.replace_all(&content, "").to_string();

    if patched == content {
        info!("No -expected-id found in esp_common.cfg, skipping patch");
        return Ok(scripts_dir.to_path_buf());
    }

    let tmp_dir = std::env::temp_dir().join("espsmith").join("openocd_scripts");
    let tmp_target = tmp_dir.join("target");
    let _ = std::fs::create_dir_all(&tmp_target);
    std::fs::write(tmp_target.join("esp_common.cfg"), &patched)
        .map_err(|e| format!("Cannot write patched esp_common.cfg: {}", e))?;

    info!("Patched -expected-id removed, overlay dir -> {}", tmp_dir.display());
    Ok(tmp_dir)
}

#[tauri::command]
pub async fn openocd_start(chip: Option<String>, probe: Option<String>) -> Result<String, String> {
    let mut guard = OPENOCD_STATE.lock().map_err(|e| e.to_string())?;
    if guard.is_some() {
        return Err("OpenOCD is already running. Stop it first with openocd_stop.".into());
    }

    let chip = chip.ok_or_else(|| "chip is required (e.g. esp32c3, esp32s3). Auto-detection removed — use project config.".to_string())?;
    info!("OpenOCD start: chip={}, probe={:?}", chip, probe);
    let chip_lower = chip.to_ascii_lowercase();
    let (target_cfg, default_interface) = chip_config(&chip_lower)?;

    // 如果指定了外部探头，使用对应的 interface 配置；否则使用默认配置
    let interface_cfg = match probe.as_deref() {
        Some(probe_name) => {
            match probe_interface_config(probe_name) {
                Some(iface) => {
                    info!("Using probe-specific interface config: {} for {}", iface, probe_name);
                    iface
                }
                None => {
                    info!("Unknown probe '{}', using default interface: {}", probe_name, default_interface);
                    default_interface
                }
            }
        }
        None => default_interface,
    };

    let openocd_bin = find_openocd_binary()?;
    let scripts_dir = find_openocd_scripts_dir()?;

    info!("Starting OpenOCD: {} for chip={}", openocd_bin.display(), chip);
    info!("  scripts_dir: {}", scripts_dir.display());
    info!("  interface: {}", interface_cfg);
    info!("  target: {}", target_cfg);

    let overlay_dir = patch_scripts_dir(&scripts_dir)?;

    let log_dir = std::env::temp_dir().join("espsmith");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("openocd.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("Cannot create OpenOCD log file: {}", e))?;

    let mut cmd = Command::new(&openocd_bin);
    cmd.args([
        "-s", &overlay_dir.to_string_lossy(),
        "-s", &scripts_dir.to_string_lossy(),
        "-f", interface_cfg,
        "-f", target_cfg,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::from(log_file));
    #[cfg(windows)]
    cmd.creation_flags(0x00000008);
    let child = cmd.spawn()
        .map_err(|e| format!("Failed to start OpenOCD ({}): {}", openocd_bin.display(), e))?;

    let pid = child.id();

    let session = OpenOcdSession {
        child,
        chip: chip_lower,
        pid,
    };

    *guard = Some(session);

    let msg = format!("OpenOCD started (PID={}) for {} — GDB port: 3333, Telnet: 4444", pid, chip);
    info!("{}", msg);
    Ok(msg)
}

#[tauri::command]
pub async fn openocd_stop() -> Result<String, String> {
    let mut guard = OPENOCD_STATE.lock().map_err(|e| e.to_string())?;
    match guard.take() {
        Some(mut session) => {
            info!("Stopping OpenOCD (PID={})", session.pid);
            let _ = session.child.kill();
            let _ = session.child.wait();
            let msg = format!("OpenOCD stopped (PID={})", session.pid);
            Ok(msg)
        }
        None => Err("OpenOCD is not running.".into()),
    }
}

#[tauri::command]
pub async fn openocd_is_running() -> Result<bool, String> {
    let mut guard = OPENOCD_STATE.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut session) = *guard {
        match session.child.try_wait() {
            Ok(Some(status)) => {
                warn!("OpenOCD process exited unexpectedly (status={})", status);
                *guard = None;
                Ok(false)
            }
            Ok(None) => Ok(true),
            Err(e) => {
                warn!("Error checking OpenOCD status: {}", e);
                *guard = None;
                Ok(false)
            }
        }
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub async fn openocd_get_chip() -> Result<String, String> {
    let guard = OPENOCD_STATE.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some(s) => Ok(s.chip.clone()),
        None => Err("OpenOCD is not running.".into()),
    }
}

pub fn is_openocd_running_sync() -> bool {
    let mut guard = match OPENOCD_STATE.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    if let Some(ref mut session) = *guard {
        match session.child.try_wait() {
            Ok(Some(_)) => { *guard = None; false }
            Ok(None) => true,
            Err(_) => { *guard = None; false },
        }
    } else {
        false
    }
}

#[allow(dead_code)] // CLI使用
pub fn get_openocd_chip_sync() -> Option<String> {
    OPENOCD_STATE.lock().ok().and_then(|g| g.as_ref().map(|s| s.chip.clone()))
}

pub fn ensure_openocd_running(chip: &str) -> Result<(), String> {
    if TcpStream::connect_timeout(
        &"127.0.0.1:4444".parse().unwrap(),
        std::time::Duration::from_millis(300),
    ).is_ok() {
        info!("OpenOCD telnet port 4444 already available");
        return Ok(());
    }

    if is_openocd_running_sync() {
        info!("OpenOCD process exists but telnet not responding, killing...");
        kill_openocd_sync();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let (target_cfg, interface_cfg) = chip_config(&chip.to_ascii_lowercase())?;

    let openocd_bin = find_openocd_binary()?;
    let scripts_dir = find_openocd_scripts_dir()?;

    info!("Auto-starting OpenOCD: {} for chip={}", openocd_bin.display(), chip);

    let overlay_dir = patch_scripts_dir(&scripts_dir)?;

    let log_dir = std::env::temp_dir().join("espsmith");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("openocd.log");
    let log_file = std::fs::File::create(&log_path)
        .map_err(|e| format!("Cannot create OpenOCD log file {:?}: {}", log_path, e))?;

    let mut cmd = Command::new(&openocd_bin);
    cmd.args([
        "-s", &overlay_dir.to_string_lossy(),
        "-s", &scripts_dir.to_string_lossy(),
        "-f", interface_cfg,
        "-f", target_cfg,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::from(log_file));
    #[cfg(windows)]
    cmd.creation_flags(0x00000008);
    let child = cmd.spawn()
        .map_err(|e| format!("Failed to start OpenOCD: {}", e))?;

    let pid = child.id();

    let mut guard = OPENOCD_STATE.lock().map_err(|e| e.to_string())?;
    *guard = Some(OpenOcdSession {
        child,
        chip: chip.to_ascii_lowercase(),
        pid,
    });

    for i in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(400));
        if TcpStream::connect_timeout(
            &"127.0.0.1:4444".parse().unwrap(),
            std::time::Duration::from_millis(200),
        ).is_ok() {
            info!("OpenOCD auto-started (PID={}), telnet ready after {}ms", pid, (i + 1) * 400);
            return Ok(());
        }
        if let Some(ref mut session) = *guard {
            if session.child.try_wait().ok().flatten().is_some() {
                info!("OpenOCD (PID={}) exited unexpectedly during startup", pid);
                *guard = None;
                let diag = read_openocd_log_tail(&log_path);
                let (diagnosis, is_fatal) = diagnose_openocd_log(&log_path);
                let fatal_prefix = if is_fatal { "[FATAL] " } else { "" };
                return Err(format!(
                    "{}OpenOCD exited unexpectedly (PID={}). Check JTAG connection.\nOpenOCD log (last 15 lines):\n{}{}",
                    fatal_prefix, pid, diag, diagnosis
                ));
            }
        }
    }

    {
        if let Ok(mut guard) = OPENOCD_STATE.lock() {
            if let Some(mut session) = guard.take() {
                let _ = session.child.kill();
                let _ = session.child.wait();
            }
        }
    }

    let diag = read_openocd_log_tail(&log_path);
    let (diagnosis, is_fatal) = diagnose_openocd_log(&log_path);
    let fatal_prefix = if is_fatal { "[FATAL] " } else { "" };
    Err(format!(
        "{}OpenOCD did not become ready within 4s (PID={}). JTAG may not be connected or chip config may be wrong.\nOpenOCD log (last 15 lines):\n{}{}",
        fatal_prefix, pid, diag, diagnosis
    ))
}

fn read_openocd_log_tail(log_path: &std::path::Path) -> String {
    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .lines()
        .rev()
        .take(15)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn diagnose_openocd_log(log_path: &std::path::Path) -> (String, bool) {
    let content = std::fs::read_to_string(log_path).unwrap_or_default();
    let mut diag = String::new();
    let mut is_fatal = false;

    if content.contains("Unsupported DTM version") || (content.contains("TAP") && content.contains("expected") && content.contains("got"))
    {
        is_fatal = true;
        diag.push_str("\n[DIAGNOSIS] FATAL: JTAG TAP ID mismatch. The connected chip is NOT the same as the configured target.\n");
        diag.push_str("This will NOT be resolved by retrying or recovery actions.\n");
        diag.push_str("DO NOT modify OpenOCD config files. Instead:\n");
        diag.push_str("  1. Check the physical chip marking on the board.\n");
        diag.push_str("  2. If the chip differs from the project target, update project config.\n");
        diag.push_str("  Known IDs: ESP32 (Xtensa) = 0x120034e5, ESP32-C3 (RISC-V) = 0x00005c25\n");
    } else if content.contains("TAP") && content.contains("expected") {
        diag.push_str("\n[DIAGNOSIS] JTAG TAP ID mismatch detected. The connected chip's JTAG ID does not match the configured target.\n");
        diag.push_str("This usually means the physical chip is NOT the same model as the project configuration.\n");
        diag.push_str("DO NOT modify OpenOCD config files. Instead:\n");
        diag.push_str("  1. Check the chip marking on the physical board.\n");
        diag.push_str("  2. If the chip is not an ESP32-C3, update the project target to match.\n");
        diag.push_str("  Known IDs: ESP32 (Xtensa) = 0x120034e5, ESP32-C3 (RISC-V) = 0x00005c25\n");
    }

    if content.contains("Error: libusb_open() failed") || content.contains("LIBUSB_ERROR") {
        diag.push_str("\n[DIAGNOSIS] USB driver issue. The JTAG interface may need WinUSB driver via Zadig.\n");
    }

    if diag.is_empty() {
        diag.push_str("\nNo specific diagnosis pattern found in OpenOCD log. Check JTAG wiring and USB connection.\n");
    }

    (diag, is_fatal)
}

pub fn kill_openocd_sync() {
    if let Ok(mut guard) = OPENOCD_STATE.lock() {
        if let Some(mut session) = guard.take() {
            info!("Killing OpenOCD (PID={})", session.pid);
            let _ = session.child.kill();
            let _ = session.child.try_wait();
        }
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/F", "/IM", "openocd.exe"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let _ = cmd.status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .arg("openocd")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[allow(dead_code)] // CLI使用
pub fn find_openocd_binary_sync() -> String {
    find_openocd_binary()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "openocd".into())
}

#[allow(dead_code)] // CLI使用
pub fn find_openocd_scripts_sync() -> String {
    find_openocd_scripts_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "share/openocd/scripts".into())
}

#[allow(dead_code)] // CLI使用
pub fn chip_config_sync(chip: &str) -> Result<(&'static str, &'static str), String> {
    chip_config(chip)
}

#[allow(dead_code)] // Pipeline恢复策略预留
pub fn probe_hard_reset_via_openocd() -> Result<String, String> {
    use std::io::{Read, Write as IoWrite};
    use std::net::TcpStream;
    use std::time::Duration;

    let mut stream = TcpStream::connect_timeout(
        &"127.0.0.1:4444".parse().unwrap(),
        Duration::from_secs(2),
    ).map_err(|e| format!("Cannot connect to OpenOCD telnet for reset: {}. Is OpenOCD running?", e))?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

    let mut buf = [0u8; 256];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(_) => break,
        }
    }

    stream.write_all(b"reset\n").map_err(|e| e.to_string())?;

    let mut output = String::new();
    let mut total_read = 0;
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                output.push_str(&String::from_utf8_lossy(&buf[..n]));
                total_read += n;
                if total_read > 4096 { break; }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    Ok(output)
}

#[allow(dead_code)] // Pipeline恢复策略预留
pub fn probe_soft_reset_via_openocd() -> Result<String, String> {
    use std::io::{Read, Write as IoWrite};
    use std::net::TcpStream;
    use std::time::Duration;

    let mut stream = TcpStream::connect_timeout(
        &"127.0.0.1:4444".parse().unwrap(),
        Duration::from_secs(2),
    ).map_err(|e| format!("Cannot connect to OpenOCD telnet for reset: {}. Is OpenOCD running?", e))?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

    let mut buf = [0u8; 256];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(_) => break,
        }
    }

    stream.write_all(b"reset halt\n").map_err(|e| e.to_string())?;

    let mut output = String::new();
    let mut total_read = 0;
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                output.push_str(&String::from_utf8_lossy(&buf[..n]));
                total_read += n;
                if total_read > 4096 { break; }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    Ok(output)
}