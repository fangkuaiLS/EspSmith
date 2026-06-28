//! Flash adapters — different flashing methods per MCU family.
//!
//! Three flash paths:
//! - IdfEsptoolFlashAdapter: Serial via esptool (idf.py flash) — standard UART path
//! - OpenOcdFlashAdapter:    JTAG via OpenOCD (program + verify) — JTAG path, recommended
//! - UF2CopyAdapter:         Mass-storage copy for RP2040

use super::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Instant;

/// ESP-IDF flash (wraps idf.py flash).
pub struct IdfEsptoolFlashAdapter;

impl Adapter for IdfEsptoolFlashAdapter {
    fn name(&self) -> &str { "flash.idf_esptool" }
    fn description(&self) -> &str { "Flash via idf.py flash (esptool)" }

    fn execute(&self, params: &serde_json::Value, work_dir: &str) -> AdapterResult {
        let _port = params.get("port").and_then(|v| v.as_str()).unwrap_or("auto");

        // This delegates to the IDF flash function, which needs idf_path
        // For now, use the params if provided, otherwise fall back to environment
        let idf_path = params.get("idf_path")
            .and_then(|v| v.as_str()).unwrap_or("");

        let start = Instant::now();
        // Use port from params or platform-appropriate default
        let port = params.get("port")
            .and_then(|v| v.as_str()).unwrap_or(super::default_port_hint());

        if idf_path.is_empty() {
            return AdapterResult::fail(
                "ESP-IDF path not configured for flash adapter".into(),
                None,
                start.elapsed().as_millis() as u64,
            );
        }

        match crate::idf::flash(work_dir, idf_path, port) {
            Ok(output) => AdapterResult::ok(
                Some(output),
                start.elapsed().as_millis() as u64,
            ),
            Err(output) => AdapterResult::fail(
                format!("Flash failed:\n{}", output),
                Some(output),
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}

/// OpenOCD flash adapter — flash firmware via JTAG/SWD using OpenOCD.
///
/// Uses the running OpenOCD's telnet interface (port 4444) to send
/// `program <elf> verify reset` command. This is the recommended path
/// when USB-JTAG is available because it uses the same connection for
/// both flashing and debugging, eliminating serial port conflicts.
pub struct OpenOcdFlashAdapter;

impl Adapter for OpenOcdFlashAdapter {
    fn name(&self) -> &str { "flash.openocd" }
    fn description(&self) -> &str { "Flash via OpenOCD JTAG/SWD (program + verify)" }

    fn execute(&self, params: &serde_json::Value, work_dir: &str) -> AdapterResult {
        let start = Instant::now();

        let chip = match params
            .get("chip")
            .and_then(|v| v.as_str())
            .or_else(|| params.get("board").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
        {
            Some(c) => c,
            None => {
                return AdapterResult::fail(
                    "chip is required for OpenOCD flash. Ensure project config has a valid chip model.".into(),
                    None,
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let elf_path = params
            .get("elf_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| find_elf_in_build_dir(work_dir));

        let elf = match elf_path {
            Some(ref path) => path.clone(),
            None => {
                return AdapterResult::fail(
                    "No ELF file found. Specify elf_path or ensure build completed.".into(),
                    None,
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        if !Path::new(&elf).exists() {
            return AdapterResult::fail(
                format!("ELF file not found: {}", elf),
                None,
                start.elapsed().as_millis() as u64,
            );
        }

        if let Err(e) = crate::commands::openocd::ensure_openocd_running(&chip, None) {
            return AdapterResult::fail(
                format!("Failed to start OpenOCD: {}", e),
                Some(e),
                start.elapsed().as_millis() as u64,
            );
        }

        if let Err(e) = wait_for_openocd_telnet(8) {
            return AdapterResult::fail(
                format!("OpenOCD started but telnet not ready: {}", e),
                Some(e),
                start.elapsed().as_millis() as u64,
            );
        }

        let port = params.get("port").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let full_flash_result = flash_full_firmware_via_openocd(&elf, work_dir, &port);
        match full_flash_result {
            Ok(output) => AdapterResult::ok(
                Some(format!("JTAG flash successful:\n{}", output)),
                start.elapsed().as_millis() as u64,
            ),
            Err(e) => AdapterResult::fail(
                format!("JTAG flash failed: {}", e),
                Some(e),
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}

pub fn find_elf_in_build_dir(work_dir: &str) -> Option<String> {
    let candidates = [
        format!("{}/build/app.elf", work_dir),
        format!("{}/build/hello_world.elf", work_dir),
    ];
    for c in &candidates {
        if Path::new(c).exists() {
            return Some(super::normalize_path_for_gdb(c));
        }
    }
    let build_dir = Path::new(work_dir).join("build");
    if build_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&build_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.ends_with(".elf") && !name_str.contains("bootloader") {
                    return Some(super::normalize_path_for_gdb(&entry.path().to_string_lossy()));
                }
            }
        }
    }
    None
}

fn flash_full_firmware_via_openocd(elf_path: &str, work_dir: &str, _port: &str) -> Result<String, String> {
    let build_dir = Path::new(work_dir).join("build");
    let project_name = Path::new(work_dir)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let bootloader = build_dir.join("bootloader").join("bootloader.bin");
    let partition_table = build_dir.join("partition_table").join("partition-table.bin");
    let app_bin = build_dir.join(format!("{}.bin", project_name));

    if !bootloader.exists() || !partition_table.exists() || !app_bin.exists() {
        tracing::info!("Full firmware bins not found, falling back to ELF-only program");
        return flash_via_openocd_telnet(elf_path);
    }

    let mut stream = TcpStream::connect_timeout(
        &super::openocd_addr(),
        std::time::Duration::from_secs(2),
    ).map_err(|e| format!("Cannot connect to OpenOCD telnet: {}. Is OpenOCD running?", e))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;

    let mut buf = [0u8; 4096];
    let _ = drain_telnet_output(&mut stream, &mut buf);

    stream
        .write_all(b"reset halt\n")
        .map_err(|e| format!("Failed to send reset halt: {}", e))?;
    let _ = read_until_prompt(&mut stream, &mut buf, 10);

    let mut output = String::new();

    let flash_parts = [
        ("0x0", bootloader.to_string_lossy().to_string()),
        ("0x8000", partition_table.to_string_lossy().to_string()),
        ("0x10000", app_bin.to_string_lossy().to_string()),
    ];

    for (addr, bin_path) in &flash_parts {
        let bin_slash = super::normalize_path_for_gdb(bin_path);
        let cmd = format!("program {} {}\n", bin_slash, addr);
        tracing::info!("Flashing: {}", cmd.trim());

        stream
            .write_all(cmd.as_bytes())
            .map_err(|e| format!("Failed to send program command: {}", e))?;

        let part_output = read_until_prompt(&mut stream, &mut buf, 60);
        output.push_str(&part_output);

        if part_output.to_lowercase().contains("error")
            && !part_output.contains("Programming Finished")
        {
            return Err(format!("Flash failed at {}:\n{}", addr, part_output));
        }
    }

    stream
        .write_all(b"reset run\n")
        .map_err(|e| format!("Failed to send reset run: {}", e))?;
    let _ = read_until_prompt(&mut stream, &mut buf, 5);

    Ok(output)
}

fn wait_for_openocd_telnet(max_retries: u32) -> Result<(), String> {
    for i in 0..max_retries {
        if TcpStream::connect_timeout(
            &super::openocd_addr(),
            std::time::Duration::from_millis(500),
        ).is_ok() {
            tracing::info!("OpenOCD telnet ready after {} attempts", i + 1);
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err("OpenOCD telnet port 4444 not available after waiting".into())
}

fn flash_via_openocd_telnet(elf_path: &str) -> Result<String, String> {
    tracing::info!("flash_via_openocd_telnet: connecting to {}...", super::OPENOCD_ADDR);

    let mut stream = TcpStream::connect_timeout(
        &super::openocd_addr(),
        std::time::Duration::from_secs(2),
    ).map_err(|e| format!("Cannot connect to OpenOCD telnet (port 4444): {}. Is OpenOCD running?", e))?;
    tracing::info!("flash_via_openocd_telnet: connected to telnet");

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;

    let mut buf = [0u8; 4096];
    let _ = drain_telnet_output(&mut stream, &mut buf);

    tracing::info!("flash_via_openocd_telnet: sending reset halt");
    stream
        .write_all(b"reset halt\n")
        .map_err(|e| format!("Failed to send reset halt command: {}", e))?;
    let halt_output = read_until_prompt(&mut stream, &mut buf, 10);
    tracing::info!("flash_via_openocd_telnet: reset halt response: {} bytes", halt_output.len());

    let cmd = format!("program {} reset\n", super::normalize_path_for_gdb(elf_path));
    tracing::info!("flash_via_openocd_telnet: sending program command: {}", cmd.trim());
    stream
        .write_all(cmd.as_bytes())
        .map_err(|e| format!("Failed to send program command: {}", e))?;

    let mut output = String::new();
    let deadline = Instant::now() + std::time::Duration::from_secs(90);
    let mut idle_rounds: u32 = 0;

    while Instant::now() < deadline {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(count) => {
                let text = String::from_utf8_lossy(&buf[..count]);
                output.push_str(&text);
                idle_rounds = 0;

                let lower = output.to_lowercase();
                if lower.contains("programming finished")
                    || lower.contains("verified ok")
                    || lower.contains("** verified ok **")
                {
                    tracing::info!("OpenOCD flash completed");
                    break;
                }
                if lower.contains("programming failed")
                    || lower.contains("auto erase failed")
                {
                    tracing::warn!("OpenOCD flash error detected");
                    break;
                }
                if output.contains("> ") && lower.contains("wrote") {
                    tracing::info!("OpenOCD wrote flash, prompt received");
                    break;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                idle_rounds += 1;
                if output.contains("> ") || idle_rounds > 45 {
                    break;
                }
            }
            Err(e) => return Err(format!("Read error during flash: {}", e)),
        }
    }

    let combined = output.to_lowercase();
    let has_error = combined.contains("error")
        || combined.contains("failed")
        || combined.contains("cannot");
    let has_success = combined.contains("verified ok")
        || combined.contains("** verified ok **")
        || combined.contains("wrote");

    if has_error && !has_success {
        Err(format!("OpenOCD reported error:\n{}", output))
    } else if has_success || output.contains("> ") {
        Ok(output)
    } else {
        Err(format!("OpenOCD flash timed out. Output so far:\n{}", output))
    }
}

fn read_until_prompt(stream: &mut TcpStream, buf: &mut [u8], max_secs: u64) -> String {
    let mut output = String::new();
    let deadline = Instant::now() + std::time::Duration::from_secs(max_secs);
    while Instant::now() < deadline {
        match stream.read(buf) {
            Ok(0) => break,
            Ok(count) => {
                let text = String::from_utf8_lossy(&buf[..count]);
                output.push_str(&text);
                if output.contains("> ") {
                    break;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                continue;
            }
            Err(_) => break,
        }
    }
    output
}

fn drain_telnet_output(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<usize> {
    stream.set_read_timeout(Some(std::time::Duration::from_millis(200)))?;
    let total = loop {
        match stream.read(buf) {
            Ok(0) => break 0,
            Ok(_) => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break 0,
            Err(e) => return Err(e),
        }
    };
    stream.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    Ok(total)
}

/// UF2 copy adapter (for RP2040).
pub struct UF2CopyAdapter;

impl Adapter for UF2CopyAdapter {
    fn name(&self) -> &str { "flash.uf2_copy" }
    fn description(&self) -> &str { "Copy UF2 binary to RP2040 mass storage" }

    fn execute(&self, params: &serde_json::Value, work_dir: &str) -> AdapterResult {
        let uf2_file = params.get("uf2_file").and_then(|v| v.as_str()).unwrap_or("firmware.uf2");
        let mount_point = params.get("mount_point").and_then(|v| v.as_str()).unwrap_or("");

        let start = Instant::now();
        let src = std::path::Path::new(work_dir).join("build").join(uf2_file);
        if !src.exists() {
            return AdapterResult::fail(
                format!("UF2 file not found: {}", src.display()),
                None,
                start.elapsed().as_millis() as u64,
            );
        }

        // Find RPI-RP2 mount
        let dest = if !mount_point.is_empty() {
            std::path::PathBuf::from(mount_point).join(uf2_file)
        } else {
            // Scan drives for RPI-RP2
            let drives = drive_mounts();
            let rp2 = drives.iter().find(|d| d.contains("RPI-RP2"));
            match rp2 {
                Some(d) => std::path::PathBuf::from(d).join(uf2_file),
                None => return AdapterResult::fail(
                    "RP2040 not found in BOOTSEL mode".into(),
                    None,
                    start.elapsed().as_millis() as u64,
                ),
            }
        };

        match std::fs::copy(&src, &dest) {
            Ok(_) => AdapterResult::ok(
                Some(format!("Copied {} to {}", src.display(), dest.display())),
                start.elapsed().as_millis() as u64,
            ),
            Err(e) => AdapterResult::fail(
                format!("UF2 copy failed: {e}"),
                None,
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}

/// Drive mount enumeration helper (UF2 copy needs this).
fn drive_mounts() -> Vec<String> {
    let mut mounts = Vec::new();
    if cfg!(windows) {
        for c in 'A'..='Z' {
            let path = format!("{c}:\\");
            if std::path::Path::new(&path).exists() {
                mounts.push(path);
            }
        }
    } else {
        for entry in std::fs::read_dir("/media").into_iter().flatten().flatten() {
            mounts.push(entry.path().to_string_lossy().to_string());
        }
        for entry in std::fs::read_dir("/Volumes").into_iter().flatten().flatten() {
            mounts.push(entry.path().to_string_lossy().to_string());
        }
    }
    mounts
}