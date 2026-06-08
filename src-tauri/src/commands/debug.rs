//! GDB 调试命令模块
//!
//! 功能：
//! - 通过 arm-none-eabi-gdb 连接 ESP32 目标
//! - 设置/删除断点
//! - 单步执行
//! - 读取变量/寄存器
//! - Core dump 分析

use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugState {
    pub running: bool,
    pub pc: String,
    pub stack: Vec<String>,
    pub locals: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: u32,
    pub file: String,
    pub line: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableInfo {
    pub name: String,
    pub value: String,
    pub type_name: String,
}

/// 执行 GDB 命令（batch 模式，每次启动新进程）
fn gdb_command(script: &str, target_chip: Option<&str>) -> Result<String, String> {
    let gdb_binary = super::gdb_session::find_gdb_binary(target_chip)?;
    let child = Command::new(&gdb_binary)
        .args(["-batch", "-nx", "-ex"])
        .arg(format!("target remote localhost:3333"))
        .arg("-ex")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("GDB not found ({gdb_binary}): {}", e))?;

    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 获取调试状态
#[tauri::command]
pub async fn get_debug_state(target_chip: Option<String>) -> Result<DebugState, String> {
    info!("Getting debug state");
    let bt = gdb_command("bt", target_chip.as_deref()).unwrap_or_default();

    let stack: Vec<String> = bt.lines()
        .filter(|l| l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();

    Ok(DebugState {
        running: true,
        pc: "0x00000000".into(),
        stack,
        locals: vec![],
    })
}

/// 设置断点
#[tauri::command]
pub async fn set_breakpoint(file: String, line: u32, target_chip: Option<String>) -> Result<Breakpoint, String> {
    info!("Setting breakpoint at {}:{}", file, line);
    let cmd = format!("break {}:{}", file, line);
    let result = gdb_command(&cmd, target_chip.as_deref())?;

    Ok(Breakpoint {
        id: 1,
        file,
        line,
        enabled: result.contains("Breakpoint"),
    })
}

/// 继续执行
#[tauri::command]
pub async fn continue_execution(target_chip: Option<String>) -> Result<(), String> {
    info!("Continuing execution");
    gdb_command("continue", target_chip.as_deref())?;
    Ok(())
}

/// 单步执行
#[tauri::command]
pub async fn step_over(target_chip: Option<String>) -> Result<(), String> {
    info!("Stepping over");
    gdb_command("next", target_chip.as_deref())?;
    Ok(())
}

/// 进入函数
#[tauri::command]
pub async fn step_into(target_chip: Option<String>) -> Result<(), String> {
    info!("Stepping into");
    gdb_command("step", target_chip.as_deref())?;
    Ok(())
}

/// 跳出函数
#[tauri::command]
pub async fn step_out(target_chip: Option<String>) -> Result<(), String> {
    info!("Stepping out");
    gdb_command("finish", target_chip.as_deref())?;
    Ok(())
}

/// 读取变量值
#[tauri::command]
pub async fn read_variable(name: String, target_chip: Option<String>) -> Result<VariableInfo, String> {
    info!("Reading variable: {}", name);
    let cmd = format!("print {}", name);
    let result = gdb_command(&cmd, target_chip.as_deref())?;

    // 解析 GDB 输出 "$1 = (type) value"
    let value = result.lines()
        .find(|l| l.contains('='))
        .map(|l| l.split('=').nth(1).unwrap_or("?").trim())
        .unwrap_or("?");

    Ok(VariableInfo {
        name,
        value: value.to_string(),
        type_name: "unknown".into(),
    })
}

/// Core dump 分析
#[tauri::command]
pub async fn analyze_coredump(
    elf_path: String,
    dump_path: String,
    target_chip: Option<String>,
) -> Result<String, String> {
    info!("Analyzing core dump: {} with ELF: {}", dump_path, elf_path);

    let gdb_binary = super::gdb_session::find_gdb_binary(target_chip.as_deref())?;

    let script = format!(
        "set confirm off\nfile {}\ncore {}\nbt\ninfo registers\nframe\nquit",
        elf_path, dump_path
    );

    let output = Command::new(&gdb_binary)
        .args(["-batch", "-nx", "-ex", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("GDB not found ({gdb_binary}): {}", e))?
        .wait_with_output()
        .map_err(|e| e.to_string())?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}