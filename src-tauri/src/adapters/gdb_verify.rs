use super::Adapter;
use crate::commands::gdb_session;
use serde_json::Value;
use std::io::{Read, Write};
use std::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct GdbSessionVerifyAdapter;

impl Adapter for GdbSessionVerifyAdapter {
    fn name(&self) -> &str { "verify.gdb_session" }
    fn description(&self) -> &str { "Verify device state via GDB (batch mode)" }

    fn execute(&self, params: &Value, work_dir: &str) -> super::AdapterResult {
        let start = Instant::now();

        // 芯片型号获取优先级：
        // 1. params 中的 target_chip
        // 2. 项目 sdkconfig 中的 CONFIG_IDF_TARGET
        // 3. 项目 .espsmith/project.json 中的 chipModel
        // 4. 兜底 esp32
        // 注意：不使用 connection::chip_hint，因为它返回的是 "ESP32-USB-JTAG"（接口类型），
        // 而不是实际芯片型号（如 esp32s3），会导致 OpenOCD 配置查找失败。
        let target_chip = params
            .get("target_chip")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                // 从项目 sdkconfig 读取
                let sdkconfig = std::path::Path::new(work_dir).join("sdkconfig");
                if let Ok(content) = std::fs::read_to_string(&sdkconfig) {
                    for line in content.lines() {
                        if let Some(target) = line.strip_prefix("CONFIG_IDF_TARGET=\"") {
                            if let Some(end) = target.find('"') {
                                return Some(target[..end].to_string());
                            }
                        }
                    }
                }
                None
            })
            .or_else(|| {
                // 从 .espsmith/project.json 读取
                let proj_json = std::path::Path::new(work_dir)
                    .join(".espsmith").join("project.json");
                if let Ok(content) = std::fs::read_to_string(&proj_json) {
                    if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(chip) = cfg["chipModel"].as_str().or_else(|| cfg["chip"].as_str()) {
                            return Some(chip.to_string());
                        }
                    }
                }
                None
            })
            .unwrap_or_else(|| "esp32".to_string());

        let elf_path = params
            .get("elf_path")
            .and_then(|v| v.as_str())
            .map(|s| super::normalize_path_for_gdb(s).to_string())
            .or_else(|| {
                let bin_name = std::path::Path::new(work_dir)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let elf = std::path::Path::new(work_dir)
                    .join("build")
                    .join(format!("{}.elf", bin_name));
                if elf.exists() {
                    Some(super::normalize_path_for_gdb(&elf.to_string_lossy()))
                } else {
                    None
                }
            });

        if let Err(e) = crate::commands::openocd::ensure_openocd_running(&target_chip) {
            return super::AdapterResult::fail(
                format!("OpenOCD not running and cannot start: {}", e),
                Some(e),
                start.elapsed().as_millis() as u64,
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(500));

        if std::net::TcpStream::connect_timeout(
            &"127.0.0.1:3333".parse().unwrap(),
            std::time::Duration::from_millis(500),
        ).is_err() {
            return super::AdapterResult::fail(
                "GDB server (port 3333) not available. OpenOCD may not have started correctly.".to_string(),
                Some("Port 3333 not reachable".to_string()),
                start.elapsed().as_millis() as u64,
            );
        }

        let gdb_binary = match gdb_session::find_gdb_binary(Some(&target_chip)) {
            Ok(path) => path,
            Err(e) => {
                return super::AdapterResult::fail(
                    format!("GDB not found: {}", e),
                    Some(e),
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let elf_arg = match elf_path {
            Some(ref path) => vec!["-ex".to_string(), format!("file {}", path)],
            None => vec![],
        };

        let mut args = vec!["-batch".to_string(), "-nx".to_string(), "-quiet".to_string()];
        args.extend(elf_arg);
        args.extend(vec![
            "-ex".to_string(), "set remotetimeout 5".to_string(),
            "-ex".to_string(), "target remote localhost:3333".to_string(),
            "-ex".to_string(), "info registers pc".to_string(),
            "-ex".to_string(), "backtrace 5".to_string(),
            "-ex".to_string(), "monitor reset run".to_string(),
        ]);

        let mut cmd = std::process::Command::new(&gdb_binary);
        cmd.args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        let spawn_result = cmd.spawn();

        let child = match spawn_result {
            Ok(c) => c,
            Err(e) => {
                return super::AdapterResult::fail(
                    format!("Failed to run GDB: {}", e),
                    Some(e.to_string()),
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = child.wait_with_output();
            let _ = tx.send(result);
        });

        let output = match rx.recv_timeout(std::time::Duration::from_secs(15)) {
            Ok(Ok(out)) => Ok(out),
            Ok(Err(e)) => Err(format!("GDB process error: {}", e)),
            Err(_) => {
                let mut cmd = std::process::Command::new("taskkill");
                cmd.args(["/F", "/T", "/PID", &pid.to_string()])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                #[cfg(windows)]
                { cmd.creation_flags(0x08000000); }
                let _ = cmd.spawn();
                std::thread::sleep(std::time::Duration::from_millis(500));
                Err("GDB process timed out after 15s".to_string())
            }
        };

        let duration = start.elapsed().as_millis() as u64;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let combined = format!("{}\n{}", stdout, stderr);

                let lower = combined.to_lowercase();
                let has_pc = lower.contains("pc") && lower.contains("0x4");
                let has_bt = lower.contains("#0") || lower.contains("frame");

                if has_pc || has_bt {
                    let mut verify_output = Vec::new();
                    for line in combined.lines() {
                        let trimmed = line.trim();
                        if trimmed.contains("pc") && trimmed.contains("0x4") {
                            verify_output.push(format!("PC: {}", trimmed));
                        }
                        if trimmed.starts_with('#') || trimmed.starts_with("frame") {
                            verify_output.push(format!("BT: {}", trimmed));
                        }
                    }
                    if verify_output.is_empty() {
                        verify_output.push("GDB connected and read device state".to_string());
                    }

                    if let Ok(mut stream) = std::net::TcpStream::connect_timeout(
                        &"127.0.0.1:4444".parse().unwrap(),
                        std::time::Duration::from_secs(1),
                    ) {
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
                        let _ = stream.write_all(b"reset run\n");
                        let mut buf = [0u8; 256];
                        let _ = stream.read(&mut buf);
                        tracing::info!("Sent reset run after GDB verify");
                    }

                    super::AdapterResult::ok(Some(verify_output.join("\n")), duration)
                } else if lower.contains("connection refused") || lower.contains("not connected") {
                    super::AdapterResult::fail(
                        "GDB cannot connect to OpenOCD (localhost:3333)".to_string(),
                        Some(combined),
                        duration,
                    )
                } else {
                    super::AdapterResult::fail(
                        format!("GDB verify failed: {}", combined),
                        Some(combined),
                        duration,
                    )
                }
            }
            Err(e) => {
                super::AdapterResult::fail(
                    format!("Failed to run GDB: {}", e),
                    Some(e),
                    duration,
                )
            }
        }
    }
}
