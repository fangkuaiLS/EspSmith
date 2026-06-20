//! 硬件配置命令模块

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::Emitter;
use tracing::info;

/// 外设实例
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeripheralInstance {
    pub id: String,
    pub definition_id: String,
    pub name: String,
    pub pin_values: HashMap<String, u32>,
    pub library_choice: String,
    #[serde(default)]
    pub notes: String,
    pub params: Option<HashMap<String, serde_json::Value>>,
}

/// 硬件配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareConfig {
    pub project_name: String,
    pub board: String,
    pub peripherals: HashMap<String, PeripheralInstance>,
}

/// 引脚冲突
#[derive(Debug, Serialize, Deserialize)]
pub struct PinConflict {
    pub pin: u32,
    pub peripheral_a: String,
    pub peripheral_b: String,
}

/// 外设更新请求（AI/用户修改配置时使用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeripheralUpdate {
    pub name: Option<String>,
    pub definition_id: Option<String>,
    pub pin_values: Option<HashMap<String, u32>>,
    pub library_choice: Option<String>,
    pub notes: Option<String>,
    pub params: Option<HashMap<String, serde_json::Value>>,
}

/// 加载硬件配置
#[tauri::command]
pub async fn get_hw_config(project_path: String) -> Result<HardwareConfig, String> {
    info!("Loading hardware config for: {}", project_path);

    let config_path = PathBuf::from(&project_path)
        .join(".espsmith")
        .join("hardware_config.json");

    if config_path.exists() {
        let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    } else {
        // 返回默认配置
        let project_dir = PathBuf::from(&project_path);
        let name = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        Ok(HardwareConfig {
            project_name: name,
            board: "ESP32-DevKit".to_string(),
            peripherals: HashMap::new(),
        })
    }
}

/// 保存硬件配置
#[tauri::command]
pub async fn save_hw_config(
    app_handle: tauri::AppHandle,
    project_path: String,
    config: HardwareConfig,
) -> Result<(), String> {
    info!("Saving hardware config for: {}", project_path);

    let config_dir = PathBuf::from(&project_path).join(".espsmith");
    fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;

    let config_path = config_dir.join("hardware_config.json");
    let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(config_path, content).map_err(|e| e.to_string())?;

    // 自动同步生成 main/hardware_pins.h
    let _ = write_hardware_header(&project_path, &config);

    let _ = app_handle.emit("hw-config-changed", &config);

    Ok(())
}

/// 检查引脚冲突
#[tauri::command]
pub async fn check_pin_conflict(
    project_path: String,
    new_instance: PeripheralInstance,
) -> Result<Vec<PinConflict>, String> {
    info!("Checking pin conflicts for: {}", new_instance.name);

    let config = get_hw_config(project_path).await?;
    let mut conflicts = Vec::new();

    for (id, existing) in &config.peripherals {
        if id == &new_instance.id {
            continue;
        }

        for (new_pin_name, new_pin_value) in &new_instance.pin_values {
            for (existing_pin_name, existing_pin_value) in &existing.pin_values {
                if new_pin_name == existing_pin_name && new_pin_value == existing_pin_value {
                    conflicts.push(PinConflict {
                        pin: *new_pin_value,
                        peripheral_a: existing.name.clone(),
                        peripheral_b: new_instance.name.clone(),
                    });
                }
            }
        }
    }

    Ok(conflicts)
}

/// 构建 C 头文件内容
fn build_header_content(config: &HardwareConfig) -> String {
    let mut header = String::new();

    header.push_str("/**\n");
    header.push_str(" * hardware_pins.h — 项目硬件引脚配置\n");
    header.push_str(" * EspSmith 自动生成，请勿手动编辑\n");
    header.push_str(" * 修改引脚请通过硬件配置面板操作\n");
    header.push_str(" */\n\n");
    header.push_str("#ifndef HARDWARE_PINS_H\n");
    header.push_str("#define HARDWARE_PINS_H\n\n");
    header.push_str("#include <stdint.h>\n\n");

    let mut instances: Vec<_> = config.peripherals.iter().collect();
    instances.sort_by_key(|(id, _)| id.to_string());

    for (id, p) in &instances {
        let safe_name = id.to_uppercase().replace('-', "_");

        header.push_str(&format!("/* ── {} ({}) ──────── */\n", p.name, p.definition_id));
        header.push_str(&format!("/* 驱动库: {} */\n", p.library_choice));
        if !p.notes.is_empty() {
            header.push_str(&format!("/* 备注: {} */\n", p.notes));
        }

        for (pin_name, pin_value) in &p.pin_values {
            let macro_name = format!("{}_{}", safe_name, pin_name.to_uppercase());
            header.push_str(&format!("#define {:<40} {}\n", macro_name, pin_value));
        }

        header.push('\n');
    }

    header.push_str("#endif // HARDWARE_PINS_H\n");

    header
}

/// 写入 pins header 到项目目录 (内部函数, 不通过 Tauri 调用)
fn write_hardware_header(project_path: &str, config: &HardwareConfig) -> Result<(), String> {
    let main_dir = PathBuf::from(project_path).join("main");
    fs::create_dir_all(&main_dir).map_err(|e| format!("创建 main 目录失败: {}", e))?;

    let header_path = main_dir.join("hardware_pins.h");
    let content = build_header_content(config);
    fs::write(&header_path, content).map_err(|e| format!("写入 {} 失败: {}", header_path.display(), e))?;

    info!("Generated hardware_pins.h at {}", header_path.display());
    Ok(())
}

/// 导出 C 头文件内容 (返回字符串)
#[tauri::command]
pub async fn export_c_header(project_path: String) -> Result<String, String> {
    info!("Exporting C header string for: {}", project_path);
    let config = get_hw_config(project_path).await?;
    Ok(build_header_content(&config))
}

/// 生成 hardware_pins.h 到项目 main/ 目录
#[tauri::command]
pub async fn generate_hardware_header(project_path: String) -> Result<(), String> {
    let config = get_hw_config(project_path.clone()).await?;
    write_hardware_header(&project_path, &config)
}

/// 获取下一个同类型外设的自增ID
#[tauri::command]
pub async fn hw_config_get_next_id(
    project_path: String,
    definition_id: String,
) -> Result<String, String> {
    let config = get_hw_config(project_path.clone()).await?;

    let mut max_num = 0u32;
    let prefix = format!("{}_", definition_id);

    for key in config.peripherals.keys() {
        if let Some(suffix) = key.strip_prefix(&prefix) {
            if let Ok(num) = suffix.parse::<u32>() {
                if num > max_num {
                    max_num = num;
                }
            }
        }
    }

    // 同时检查是否已有不带数字后缀的实例
    let base_exists = config.peripherals.contains_key(&definition_id);

    if max_num == 0 && !base_exists {
        return Ok(definition_id.clone());
    }

    Ok(format!("{}_{}", definition_id, max_num + 1))
}

/// 添加外设实例
#[tauri::command]
pub async fn hw_config_add_peripheral(
    app_handle: tauri::AppHandle,
    project_path: String,
    peripheral: PeripheralInstance,
) -> Result<HardwareConfig, String> {
    info!("Adding peripheral: {} (id: {})", peripheral.name, peripheral.id);

    let mut config = get_hw_config(project_path.clone()).await?;

    if config.peripherals.contains_key(&peripheral.id) {
        return Err(format!("外设ID '{}' 已存在", peripheral.id));
    }

    config.peripherals.insert(peripheral.id.clone(), peripheral);
    save_hw_config(app_handle, project_path, config.clone()).await?;

    Ok(config)
}

/// 更新外设实例
#[tauri::command]
pub async fn hw_config_update_peripheral(
    app_handle: tauri::AppHandle,
    project_path: String,
    id: String,
    update: PeripheralUpdate,
) -> Result<HardwareConfig, String> {
    info!("Updating peripheral: {}", id);

    let mut config = get_hw_config(project_path.clone()).await?;

    let instance = config
        .peripherals
        .get_mut(&id)
        .ok_or_else(|| format!("外设 '{}' 不存在", id))?;

    if let Some(ref name) = update.name {
        instance.name = name.clone();
    }
    if let Some(ref definition_id) = update.definition_id {
        instance.definition_id = definition_id.clone();
    }
    if let Some(ref pin_values) = update.pin_values {
        instance.pin_values = pin_values.clone();
    }
    if let Some(ref library_choice) = update.library_choice {
        instance.library_choice = library_choice.clone();
    }
    if let Some(ref notes) = update.notes {
        instance.notes = notes.clone();
    }
    if let Some(ref params) = update.params {
        instance.params = Some(params.clone());
    }

    save_hw_config(app_handle, project_path, config.clone()).await?;

    Ok(config)
}

/// 删除外设实例
#[tauri::command]
pub async fn hw_config_remove_peripheral(
    app_handle: tauri::AppHandle,
    project_path: String,
    id: String,
) -> Result<HardwareConfig, String> {
    info!("Removing peripheral: {}", id);

    let mut config = get_hw_config(project_path.clone()).await?;

    if !config.peripherals.contains_key(&id) {
        return Err(format!("外设 '{}' 不存在", id));
    }

    config.peripherals.remove(&id);
    save_hw_config(app_handle, project_path, config.clone()).await?;

    Ok(config)
}

/// 将硬件配置转为可读文本 (供 AI/调试查询)
#[tauri::command]
pub async fn hw_config_to_prompt(project_path: String) -> Result<String, String> {
    let config = get_hw_config(project_path).await?;

    if config.peripherals.is_empty() {
        return Ok("当前项目暂无外设配置。".to_string());
    }

    let mut lines = vec![
        "【项目硬件配置表】".to_string(),
        format!("开发板: {}", config.board),
        format!("header文件: main/hardware_pins.h (可直接 #include 使用)"),
        String::new(),
    ];

    for (id, p) in &config.peripherals {
        lines.push(format!("--- {} (ID: {}) ---", p.name, id));
        lines.push(format!("  类型: {}", p.definition_id));
        lines.push(format!("  驱动库: {}", p.library_choice));

        let pin_str: Vec<String> = p
            .pin_values
            .iter()
            .map(|(k, v)| format!("{}→GPIO{}", k, v))
            .collect();
        if !pin_str.is_empty() {
            lines.push(format!("  引脚: {}", pin_str.join(", ")));
        }

        if !p.notes.is_empty() {
            lines.push(format!("  备注: {}", p.notes));
        }

        lines.push(String::new());
    }

    lines.push("建议直接用 read_file 读取 main/hardware_pins.h 获取最新引脚宏定义".to_string());

    Ok(lines.join("\n"))
}
