//! 编译与烧录命令模块
//!
//! 所有命令均委托给 idf.rs 中经过 EIM 环境适配的实现，
//! 确保 idf.py 在正确的 Python 虚拟环境和工具链 PATH 下运行。

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, error};

/// 编译错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileError {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub error_type: String,
    pub message: String,
}

/// 编译结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub success: bool,
    pub output: String,
    pub errors: Vec<CompileError>,
}

/// 烧录结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashResult {
    pub success: bool,
    pub output: String,
}

/// 执行编译（通过 Adapter 层）
#[tauri::command]
pub async fn build_project(project_path: String, idf_path: String) -> Result<BuildResult, String> {
    info!("Building project: {} (idf: {})", project_path, idf_path);

    let registry = crate::adapters::create_idf_registry(&idf_path);
    let ar = crate::adapters::resolve_and_execute(
        &registry,
        "build.idf",
        &json!({}),
        &project_path,
        &idf_path,
    );

    let output = ar.stdout.clone().or(ar.stderr.clone()).unwrap_or_default();
    let errors = parse_compile_errors(&output);
    Ok(BuildResult { success: ar.success, output, errors })
}

/// 写入文件并编译（AI 自动修复用）
#[tauri::command]
pub async fn write_and_build(
    file_path: String,
    content: String,
    idf_path: String,
) -> Result<BuildResult, String> {
    info!("Writing file and building: {}", file_path);

    // 写入文件
    std::fs::write(&file_path, &content).map_err(|e| {
        error!("Failed to write file: {}", e);
        e.to_string()
    })?;

    // 推断项目目录（向上查找 CMakeLists.txt）
    let project_dir = find_project_dir(&file_path)?;
    build_project(project_dir, idf_path).await
}

/// 烧录项目（通过 Adapter 层）
#[tauri::command]
pub async fn flash_project(
    project_path: String,
    port: String,
    idf_path: String,
) -> Result<FlashResult, String> {
    info!("Flashing project: {} to port {} (idf: {})", project_path, port, idf_path);

    let registry = crate::adapters::create_idf_registry(&idf_path);
    let ar = crate::adapters::resolve_and_execute(
        &registry,
        "flash.idf_esptool",
        &json!({ "port": &port }),
        &project_path,
        &idf_path,
    );

    let output = ar.stdout.clone().or(ar.stderr.clone()).unwrap_or_default();
    Ok(FlashResult { success: ar.success, output })
}

/// 获取编译错误（AI 可解析）
#[tauri::command]
pub async fn get_build_errors(project_path: String, idf_path: String) -> Result<Vec<CompileError>, String> {
    info!("Getting build errors for: {}", project_path);

    let result = build_project(project_path, idf_path).await?;
    Ok(result.errors)
}

/// 从文件路径向上查找项目根目录（包含 CMakeLists.txt 的目录）
fn find_project_dir(file_path: &str) -> Result<String, String> {
    let mut path = std::path::PathBuf::from(file_path);
    // 如果是文件，从父目录开始
    if path.is_file() || !path.exists() {
        path = path.parent().map(|p| p.to_path_buf()).unwrap_or(path);
    }
    // 向上查找 CMakeLists.txt
    loop {
        if path.join("CMakeLists.txt").exists() {
            return Ok(path.to_string_lossy().to_string());
        }
        if let Some(parent) = path.parent() {
            path = parent.to_path_buf();
        } else {
            break;
        }
    }
    // 回退：使用文件所在目录
    let fallback = std::path::Path::new(file_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    Err(format!("Cannot find project root (no CMakeLists.txt found). File is in: {}", fallback))
}

/// 解析编译输出中的错误
fn parse_compile_errors(output: &str) -> Vec<CompileError> {
    let mut errors = Vec::new();

    // GCC 错误格式: file:line:column: error: message
    // 例如: /path/to/file.c:10:5: error: 'xxx' undeclared
    let re = regex::Regex::new(r"([^:]+):(\d+):(\d+):\s+(error|warning):\s+(.+)")
        .unwrap_or_else(|_| {
            regex::Regex::new(r"error").unwrap()
        });

    for line in output.lines() {
        if let Some(caps) = re.captures(line) {
            if caps.len() >= 6 {
                let file = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                let line_num: u32 = caps.get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(0);
                let column: u32 = caps.get(3)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(0);
                let error_type = caps.get(4).map(|m| m.as_str()).unwrap_or("error").to_string();
                let message = caps.get(5).map(|m| m.as_str()).unwrap_or("").to_string();

                errors.push(CompileError {
                    file,
                    line: line_num,
                    column,
                    error_type,
                    message,
                });
            }
        }
    }

    errors
}