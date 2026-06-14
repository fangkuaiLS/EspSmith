//! 文件系统命令模块

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Emitter;
use tracing::{debug, info, warn};

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "svgz",
    "ttf", "otf", "woff", "woff2", "bdf", "fon",
    "exe", "dll", "so", "dylib", "bin", "obj", "o", "a", "lib",
    "zip", "tar", "gz", "bz2", "xz", "7z", "rar",
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "pyc", "pyo", "class", "elf", "wasm",
    "mp3", "wav", "ogg", "flac", "mp4", "avi", "mkv", "webm",
    "dat", "dmg", "iso",
    "odg", "xcf", "psd", "ai", "eps", "svg",
    "pdb", "ilk", "exp", "map", "hex", "uf2",
    "ds_store", "db", "sqlite", "sqlite3",
];

pub fn is_binary_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// 文件条目
#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// 读取文件内容
#[tauri::command]
pub async fn read_file(path: String) -> Result<String, String> {
    debug!("Reading file: {}", path);
    let file_path = Path::new(&path);
    if is_binary_ext(file_path) {
        return Err(format!("Skipped binary file: {}", path));
    }
    let bytes = fs::read(&path).map_err(|e| {
        warn!("Failed to read file {}: {}", path, e);
        e.to_string()
    })?;
    String::from_utf8(bytes).map_err(|e| {
        warn!("File {} is not valid UTF-8: {}", path, e);
        format!("文件不是有效的 UTF-8 编码: {}", path)
    })
}

/// 写入文件内容
/// safe_mode: 为 AI 修改开启审核区
#[tauri::command]
pub async fn write_file(
    app_handle: tauri::AppHandle,
    path: String,
    content: String,
    safe_mode: bool,
) -> Result<(), String> {
    info!("Writing file: {} (safe_mode: {})", path, safe_mode);

    let file_path = PathBuf::from(&path);
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if file_name.eq_ignore_ascii_case("hardware_pins.h") {
        return Err(
            "hardware_pins.h 是自动生成的文件，禁止直接修改。\n\
             请通过硬件配置面板修改 .espsmith/hardware_config.json 来更新引脚配置。"
                .to_string(),
        );
    }

    if safe_mode {
        if let Some(parent) = file_path.parent() {
            let review_dir = parent.join(".espsmith").join("review");
            fs::create_dir_all(&review_dir).map_err(|e| e.to_string())?;

            let review_path = review_dir.join(file_name);
            fs::write(&review_path, &content).map_err(|e| e.to_string())?;

            info!("File written to review area: {}", review_path.display());
        }
    } else {
        fs::write(&path, &content).map_err(|e| {
            warn!("Failed to write file {}: {}", path, e);
            e.to_string()
        })?;
    }

    if file_name == "hardware_config.json" {
        if let Some(parent) = file_path.parent() {
            if parent.file_name().and_then(|n| n.to_str()) == Some(".espsmith") {
                if let Some(project_dir) = parent.parent() {
                    if let Ok(config_str) = fs::read_to_string(&file_path) {
                        if let Ok(config) =
                            serde_json::from_str::<crate::commands::hardware::HardwareConfig>(
                                &config_str,
                            )
                        {
                            let _ = crate::commands::hardware::generate_hardware_header(
                                project_dir.to_string_lossy().to_string(),
                            )
                            .await;
                            let _ = app_handle.emit("hw-config-changed", &config);
                            info!("Emitted hw-config-changed after config file write");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// 列出目录内容
#[tauri::command]
pub async fn list_directory(path: String) -> Result<Vec<FileEntry>, String> {
    debug!("Listing directory: {}", path);

    let entries = fs::read_dir(&path).map_err(|e| e.to_string())?;
    let mut files: Vec<FileEntry> = Vec::new();

    for entry in entries.flatten() {
        let metadata = entry.metadata().ok();
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = metadata.map(|m| m.len()).unwrap_or(0);

        if let Some(name) = entry.file_name().to_str() {
            // 跳过隐藏文件（除了 .espsmith）
            if name.starts_with('.') && name != ".espsmith" {
                continue;
            }

            files.push(FileEntry {
                name: name.to_string(),
                path: entry.path().to_string_lossy().to_string(),
                is_dir,
                size,
            });
        }
    }

    // 按目录优先排序
    files.sort_by(|a, b| {
        if a.is_dir == b.is_dir {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else if a.is_dir {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    Ok(files)
}

/// 创建新文件（带默认模板内容）
#[tauri::command]
pub async fn create_file(
    parent_path: String,
    name: String,
    content: String,
) -> Result<FileEntry, String> {
    let file_path = PathBuf::from(&parent_path).join(&name);
    info!("Creating file: {}", file_path.display());

    if file_path.exists() {
        return Err(format!("File already exists: {}", file_path.display()));
    }

    // 如果未提供内容，使用默认模板
    let file_content = if content.is_empty() {
        // 根据扩展名生成模板
        if name.ends_with(".c") || name.ends_with(".cpp") {
            format!(
                "/**\n * {name}\n * Auto-generated by EspSmith\n */\n\n#include <stdio.h>\n\nvoid app_main(void) {{\n    printf(\"Hello from EspSmith!\\n\");\n}}\n",
                name = name
            )
        } else if name.ends_with(".h") {
            format!(
                "/**\n * {name}\n * Auto-generated by EspSmith\n */\n\n#ifndef {guard}_H\n#define {guard}_H\n\n#endif /* {guard}_H */\n",
                name = name,
                guard = name.replace('.', "_").to_uppercase(),
            )
        } else {
            String::new()
        }
    } else {
        content
    };

    fs::write(&file_path, &file_content).map_err(|e| {
        warn!("Failed to create file {}: {}", file_path.display(), e);
        e.to_string()
    })?;

    let metadata = match fs::metadata(&file_path) {
        Ok(m) => m,
        Err(e) => {
            return Err(format!("Failed to stat file: {}", e));
        }
    };

    Ok(FileEntry {
        name,
        path: file_path.to_string_lossy().to_string(),
        is_dir: false,
        size: metadata.len(),
    })
}

/// 创建新文件夹
#[tauri::command]
pub async fn create_folder(
    parent_path: String,
    name: String,
) -> Result<FileEntry, String> {
    let dir_path = PathBuf::from(&parent_path).join(&name);
    info!("Creating folder: {}", dir_path.display());

    if dir_path.exists() {
        return Err(format!("Folder already exists: {}", dir_path.display()));
    }

    fs::create_dir_all(&dir_path).map_err(|e| {
        warn!("Failed to create folder {}: {}", dir_path.display(), e);
        e.to_string()
    })?;

    Ok(FileEntry {
        name,
        path: dir_path.to_string_lossy().to_string(),
        is_dir: true,
        size: 0,
    })
}

/// 重命名文件或文件夹
#[tauri::command]
pub async fn rename_file(
    old_path: String,
    new_name: String,
) -> Result<FileEntry, String> {
    let old = PathBuf::from(&old_path);
    info!("Renaming: {} -> {}", old.display(), new_name);

    if !old.exists() {
        return Err(format!("File not found: {}", old.display()));
    }

    let parent = old.parent().ok_or("Cannot determine parent directory")?;
    let new_path = parent.join(&new_name);

    if new_path.exists() {
        return Err(format!("A file named '{}' already exists", new_name));
    }

    let is_dir = old.is_dir();
    fs::rename(&old, &new_path).map_err(|e| {
        warn!("Failed to rename {}: {}", old.display(), e);
        e.to_string()
    })?;

    let metadata = fs::metadata(&new_path).map_err(|e| e.to_string())?;

    Ok(FileEntry {
        name: new_name,
        path: new_path.to_string_lossy().to_string(),
        is_dir,
        size: metadata.len(),
    })
}

/// 删除文件或文件夹（递归删除目录）
#[tauri::command]
pub async fn delete_file(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    info!("Deleting: {}", target.display());

    if !target.exists() {
        return Err(format!("File not found: {}", target.display()));
    }

    if target.is_dir() {
        fs::remove_dir_all(&target).map_err(|e| {
            warn!("Failed to delete directory {}: {}", target.display(), e);
            e.to_string()
        })?;
    } else {
        fs::remove_file(&target).map_err(|e| {
            warn!("Failed to delete file {}: {}", target.display(), e);
            e.to_string()
        })?;
    }

    Ok(())
}

/// 复制文件
#[tauri::command]
pub async fn duplicate_file(path: String) -> Result<FileEntry, String> {
    let original = PathBuf::from(&path);
    info!("Duplicating: {}", original.display());

    if !original.exists() {
        return Err(format!("File not found: {}", original.display()));
    }

    let stem = original.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("copy");
    let ext = original.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let parent = original.parent().ok_or("Cannot determine parent")?;

    // 生成不重复的名称：file - Copy.c, file - Copy 2.c, ...
    let mut copy_name: String;
    let mut counter: u32 = 1;
    loop {
        if counter == 1 {
            copy_name = if ext.is_empty() {
                format!("{} - Copy", stem)
            } else {
                format!("{} - Copy.{}", stem, ext)
            };
        } else {
            copy_name = if ext.is_empty() {
                format!("{} - Copy {}", stem, counter)
            } else {
                format!("{} - Copy {}.{}", stem, counter, ext)
            };
        }
        if !parent.join(&copy_name).exists() {
            break;
        }
        counter += 1;
    }

    let dest = parent.join(&copy_name);
    fs::copy(&original, &dest).map_err(|e| {
        warn!("Failed to duplicate {}: {}", original.display(), e);
        e.to_string()
    })?;

    let metadata = fs::metadata(&dest).map_err(|e| e.to_string())?;

    Ok(FileEntry {
        name: copy_name,
        path: dest.to_string_lossy().to_string(),
        is_dir: false,
        size: metadata.len(),
    })
}

/// 搜索结果条目
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchMatch {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
}

/// 在项目文件中搜索文本
#[tauri::command]
pub async fn search_in_files(project_path: String, query: String) -> Result<Vec<SearchMatch>, String> {
    use std::io::{BufRead, BufReader};

    if query.is_empty() {
        return Ok(Vec::new());
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    let max_results = 500;

    fn visit_dir(
        dir: &std::path::Path,
        query_lower: &str,
        results: &mut Vec<SearchMatch>,
        max_results: usize,
    ) -> Result<(), String> {
        if results.len() >= max_results {
            return Ok(());
        }

        let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;
        for entry in entries {
            if results.len() >= max_results {
                break;
            }
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let metadata = entry.metadata().ok();

            if let Some(meta) = &metadata {
                if meta.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if name.starts_with('.') && name != ".espsmith" {
                        continue;
                    }
                    visit_dir(&path, query_lower, results, max_results)?;
                    continue;
                }

                // 跳过大文件（> 1MB）
                if meta.len() > 1_000_000 {
                    continue;
                }

                // 跳过二进制文件（按扩展名判断）
                if is_binary_ext(&path) {
                    continue;
                }
            }

            // 尝试读取并搜索文件
            let file = match fs::File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };

            let reader = BufReader::new(file);
            for (line_idx, line) in reader.lines().enumerate() {
                if results.len() >= max_results {
                    break;
                }
                let Ok(line_content) = line else { continue; };
                if line_content.to_lowercase().contains(query_lower) {
                    results.push(SearchMatch {
                        file_path: path.to_string_lossy().to_string(),
                        line_number: line_idx + 1,
                        line_content,
                    });
                }
            }
        }
        Ok(())
    }

    let root = std::path::Path::new(&project_path);
    visit_dir(root, &query_lower, &mut results, max_results)?;
    Ok(results)
}
