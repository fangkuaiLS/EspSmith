//! 项目管理命令模块
//!
//! 功能：
//! - create_project: 通过 idf.py create-project 初始化项目
//! - 使用用户配置的 IDF 路径，自动设置芯片目标
//! - 自动生成 .espsmith 元信息
//! - 项目级芯片/串口持久化（参考官方 vscode-esp-idf-extension）

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub path: String,
    pub chip: String,
    pub idf_path: String,
    #[serde(default)]
    pub flash_size: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub chip: String,
    pub idf_version: String,
    pub has_hardware_config: bool,
}

/// 项目持久化配置（存在 .espsmith/project.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPersistedConfig {
    pub chip: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub flash_port: Option<String>,
}

// ==================== Tauri Commands ====================

#[tauri::command]
pub async fn create_project(config: ProjectConfig) -> Result<String, String> {
    info!("Creating project: {} at {} (chip: {}, flash: {:?})",
          config.name, config.path, config.chip, config.flash_size);

    // 确保父目录存在
    let parent_dir = PathBuf::from(&config.path);
    if !parent_dir.exists() {
        std::fs::create_dir_all(&parent_dir).map_err(|e| format!("无法创建父目录 {}: {}", config.path, e))?;
    }

    let project_dir = parent_dir.join(&config.name);

    // 检查项目目录是否已存在且不为空
    if project_dir.exists() {
        let is_empty = std::fs::read_dir(&project_dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(format!(
                "目录 {} 不为空，请清空目录或选择其他路径",
                project_dir.display()
            ));
        }
        std::fs::remove_dir(&project_dir).map_err(|e| format!("无法删除空目录: {}", e))?;
    }

    // 手动创建项目骨架文件（不依赖 idf.py create-project，更快更可靠）
    std::fs::create_dir_all(&project_dir).map_err(|e| format!("无法创建项目目录: {}", e))?;
    std::fs::create_dir_all(project_dir.join("main")).map_err(|e| format!("无法创建 main 目录: {}", e))?;

    let target = chip_to_target(&config.chip);

    // 顶层 CMakeLists.txt
    let top_cmake = format!(
        r#"cmake_minimum_required(VERSION 3.16)
include($ENV{{IDF_PATH}}/tools/cmake/project.cmake)
project({})
"#,
        config.name
    );
    std::fs::write(project_dir.join("CMakeLists.txt"), &top_cmake)
        .map_err(|e| format!("无法写入 CMakeLists.txt: {}", e))?;

    // main/CMakeLists.txt
    std::fs::write(project_dir.join("main").join("CMakeLists.txt"),
        "idf_component_register(SRCS \"main.c\" INCLUDE_DIRS \".\")\n")
        .map_err(|e| format!("无法写入 main/CMakeLists.txt: {}", e))?;

    // main/main.c
    std::fs::write(project_dir.join("main").join("main.c"),
        format!("#include <stdio.h>\n\nvoid app_main(void)\n{{\n    printf(\"Hello from {}!\\n\");\n}}\n", config.name))
        .map_err(|e| format!("无法写入 main/main.c: {}", e))?;

    info!("Project skeleton created: {}", project_dir.display());

    // 写入 sdkconfig.defaults（目标芯片 + Flash 大小，不立即编译）
    write_sdkconfig_defaults(&project_dir, &target, config.flash_size.as_deref())?;

    // 创建 .espsmith 元信息目录
    let meta_dir = project_dir.join(".espsmith");
    std::fs::create_dir_all(&meta_dir).map_err(|e| e.to_string())?;

    // 项目元信息
    let idf_version = crate::idf::get_idf_version(&config.idf_path);
    let meta = serde_json::json!({
        "name": config.name,
        "chip": config.chip,
        "target": target,
        "flash_size": config.flash_size,
        "idf_version": idf_version,
        "idf_path": config.idf_path,
        "created": chrono::Local::now().to_rfc3339(),
    });
    std::fs::write(
        meta_dir.join("project.json"),
        serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
    ).map_err(|e| e.to_string())?;

    info!("Project created: {}", project_dir.display());
    Ok(project_dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_project(path: String) -> Result<ProjectInfo, String> {
    info!("Opening project: {}", path);
    let project_dir = PathBuf::from(&path);
    if !project_dir.exists() {
        return Err(format!("Project directory does not exist: {}", path));
    }

    let name = project_dir.file_name()
        .and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
    let has_hardware_config = project_dir.join(".espsmith").join("hardware_config.json").exists();
    info!("Project name: {}, has_hw_config: {}", name, has_hardware_config);

    let (chip, idf_ver) = read_project_meta(&project_dir);
    info!("Project meta: chip={}, idf_ver={}", chip, idf_ver);

    let persisted = read_persisted_config(&project_dir);
    info!("Persisted config: target={:?}, flash_port={:?}", persisted.target, persisted.flash_port);

    // Sync chip to AI backend using the display name (e.g. "ESP32-S3"),
    // NOT the IDF target format (e.g. "esp32s3"), because the frontend
    // chip dropdown uses display names as option values.
    if !persisted.chip.is_empty() {
        crate::ai_assistant::sync_target_chip(persisted.chip.clone()).await;
    } else if let Some(ref target) = persisted.target {
        crate::ai_assistant::sync_target_chip(target.clone()).await;
    }
    if let Some(ref port) = persisted.flash_port {
        crate::ai_assistant::sync_flash_port(port.clone()).await;
    }

    let result = ProjectInfo { name, path, chip, idf_version: idf_ver, has_hardware_config };
    info!("Project opened successfully: {:?}", result.path);
    Ok(result)
}

#[tauri::command]
pub async fn get_project_info(path: String) -> Result<ProjectInfo, String> {
    open_project(path).await
}

/// 保存项目级配置（芯片型号 + 串口）到 .espsmith/project.json
#[tauri::command]
pub async fn save_project_config(project_path: String, chip: Option<String>, target: Option<String>, flash_port: Option<String>) -> Result<(), String> {
    let project_dir = PathBuf::from(&project_path);
    let meta_path = project_dir.join(".espsmith").join("project.json");

    // 读取现有配置
    let mut meta: serde_json::Value = if meta_path.exists() {
        let content = std::fs::read_to_string(&meta_path)
            .map_err(|e| format!("Failed to read project.json: {}", e))?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        std::fs::create_dir_all(project_dir.join(".espsmith"))
            .map_err(|e| format!("Failed to create .espsmith dir: {}", e))?;
        serde_json::json!({})
    };

    if let Some(ref c) = chip {
        meta["chip"] = serde_json::json!(c);
    }
    // Always convert chip display name to IDF target format for the "target" field
    if let Some(ref t) = target {
        meta["target"] = serde_json::json!(chip_to_target(t));
    }
    if let Some(ref p) = flash_port {
        meta["flash_port"] = serde_json::json!(p);
    }

    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    // 同步到 AI 后端 — 使用显示名称格式（与前端下拉框一致）
    if let Some(t) = target {
        crate::ai_assistant::sync_target_chip(t).await;
    }
    if let Some(p) = flash_port {
        crate::ai_assistant::sync_flash_port(p).await;
    }

    info!("Project config saved: {:?}", meta_path);
    Ok(())
}

/// 加载项目级持久化配置
#[tauri::command]
pub async fn load_project_config(project_path: String) -> Result<ProjectPersistedConfig, String> {
    let project_dir = PathBuf::from(&project_path);
    Ok(read_persisted_config(&project_dir))
}

fn chip_to_target(chip: &str) -> String {
    let lower = chip.to_lowercase();
    match lower.as_str() {
        "esp32" => "esp32".to_string(),
        "esp32-s2" => "esp32s2".to_string(),
        "esp32-s3" => "esp32s3".to_string(),
        "esp32-c3" => "esp32c3".to_string(),
        "esp32-c6" => "esp32c6".to_string(),
        other => other.to_string(),
    }
}

fn read_project_meta(project_dir: &PathBuf) -> (String, String) {
    let meta_path = project_dir.join(".espsmith").join("project.json");
    if let Ok(content) = std::fs::read_to_string(&meta_path) {
        if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
            let chip = meta.get("chip").and_then(|v| v.as_str()).unwrap_or("ESP32").to_string();
            let ver = meta.get("idf_version").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
            return (chip, ver);
        }
    }

    // 尝试从 sdkconfig 自动检测芯片型号
    let chip = detect_target_from_sdkconfig(project_dir)
        .map(target_to_display)
        .unwrap_or_else(|| "ESP32".into());

    (chip, "unknown".into())
}

/// 读取持久化配置（芯片 target + flash_port）
fn read_persisted_config(project_dir: &PathBuf) -> ProjectPersistedConfig {
    let meta_path = project_dir.join(".espsmith").join("project.json");
    let mut config = ProjectPersistedConfig {
        chip: "ESP32".into(),
        target: None,
        flash_port: None,
    };

    if let Ok(content) = std::fs::read_to_string(&meta_path) {
        if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
            config.chip = meta.get("chip").and_then(|v| v.as_str()).unwrap_or("ESP32").to_string();
            config.target = meta.get("target").and_then(|v| v.as_str()).map(|s| s.to_string());
            config.flash_port = meta.get("flash_port").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
    }

    // 如果 target 为空，尝试从 sdkconfig 检测
    if config.target.is_none() {
        config.target = detect_target_from_sdkconfig(project_dir);
    }

    config
}

/// 从 sdkconfig 中解析 CONFIG_IDF_TARGET 自动检测芯片型号
fn detect_target_from_sdkconfig(project_dir: &Path) -> Option<String> {
    let sdkconfig_path = project_dir.join("sdkconfig");
    if !sdkconfig_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&sdkconfig_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("CONFIG_IDF_TARGET=") {
            let val = trimmed.strip_prefix("CONFIG_IDF_TARGET=").unwrap_or("").trim_matches('"').to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// target 名称 → 显示名称
fn target_to_display(target: String) -> String {
    match target.as_str() {
        "esp32" => "ESP32".into(),
        "esp32s2" => "ESP32-S2".into(),
        "esp32s3" => "ESP32-S3".into(),
        "esp32c3" => "ESP32-C3".into(),
        "esp32c6" => "ESP32-C6".into(),
        other => other.to_uppercase(),
    }
}

/// 从 IDF 示例模板创建项目（参考官方 newProject 面板）
#[tauri::command]
pub async fn create_project_from_template(
    name: String,
    parent_path: String,
    chip: String,
    idf_path: String,
    template: Option<String>,
) -> Result<String, String> {
    // 验证父目录路径
    let parent = PathBuf::from(&parent_path);
    if !parent.exists() {
        std::fs::create_dir_all(&parent).map_err(|e| format!("无法创建父目录 {}: {}", parent_path, e))?;
    }

    let project_dir = parent.join(&name);
    if project_dir.exists() {
        // 检查目录是否为空
        let is_empty = std::fs::read_dir(&project_dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(format!(
                "目录 {} 不为空，请清空目录或选择其他路径",
                project_dir.display()
            ));
        }
        // 目录存在但为空，删除后重新创建
        std::fs::remove_dir(&project_dir).map_err(|e| format!("无法删除空目录: {}", e))?;
    }

    if let Some(ref tpl_name) = template {
        if tpl_name != "blank" {
            // Copy from IDF examples
            let template_path = Path::new(&idf_path).join("examples").join(tpl_name);
            if template_path.exists() {
                copy_dir_recursive(&template_path, &project_dir)
                    .map_err(|e| format!("Failed to copy template: {}", e))?;
            } else {
                return Err(format!("Template not found: {}", tpl_name));
            }
        } else {
            // Blank project: create with idf.py create-project
            crate::idf::idf_create_project(&parent_path, &name, &idf_path)?;
        }
    } else {
        // Default: blank project
        crate::idf::idf_create_project(&parent_path, &name, &idf_path)?;
    }

    // Set target chip
    let target = chip_to_target(&chip);
    crate::idf::set_target(
        project_dir.to_str().unwrap_or("."),
        &idf_path,
        &target,
    )?;

    // Create metadata
    let meta_dir = project_dir.join(".espsmith");
    std::fs::create_dir_all(&meta_dir).map_err(|e| e.to_string())?;
    let idf_version = crate::idf::get_idf_version(&idf_path);
    let meta = serde_json::json!({
        "name": name,
        "chip": chip,
        "target": target,
        "idf_version": idf_version,
        "idf_path": &idf_path,
        "template": template,
        "created": chrono::Local::now().to_rfc3339(),
    });
    std::fs::write(
        meta_dir.join("project.json"),
        serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
    ).map_err(|e| e.to_string())?;

    info!("Project created from template: {}", project_dir.display());
    Ok(project_dir.to_string_lossy().to_string())
}

fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn write_sdkconfig_defaults(project_dir: &Path, target: &str, flash_size: Option<&str>) -> Result<(), String> {
    let defaults_path = project_dir.join("sdkconfig.defaults");
    let mut lines = vec![
        format!("# Auto-generated by espsmith"),
        format!("CONFIG_IDF_TARGET=\"{}\"", target),
        format!(""),
    ];

    if let Some(flash) = flash_size {
        let size_line = match flash {
            "2MB" => "CONFIG_ESPTOOLPY_FLASHSIZE_2MB=y",
            "4MB" => "CONFIG_ESPTOOLPY_FLASHSIZE_4MB=y",
            "8MB" => "CONFIG_ESPTOOLPY_FLASHSIZE_8MB=y",
            "16MB" => "CONFIG_ESPTOOLPY_FLASHSIZE_16MB=y",
            _ => "",
        };
        if !size_line.is_empty() {
            lines.push(size_line.to_string());
        }
    }

    std::fs::write(&defaults_path, lines.join("\n") + "\n")
        .map_err(|e| format!("无法写入 sdkconfig.defaults: {}", e))?;
    info!("sdkconfig.defaults written to {}", defaults_path.display());
    Ok(())
}