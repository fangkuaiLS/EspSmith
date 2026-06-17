//! ESP-IDF 工具封装模块
//!
//! 功能：
//! - 自动检测 ESP-IDF 安装位置（含 EIM/VSCode 扩展安装方式）
//! - 用户配置的路径保存
//! - 执行 idf.py 命令（通过虚拟环境 Python，不再依赖 export.bat）

use std::io::{BufRead, BufReader};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tracing::{info, warn};

/// ESP-IDF 环境信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IDFEnvironment {
    pub idf_path: String,
    pub version: String,
    pub tools_path: String,
    pub python_path: Option<String>,
    pub source: DetectionSource,
}

/// 检测来源
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DetectionSource {
    AutoDetected,
    UserConfigured,
    EnvVariable,
    EimDiscovered,
    #[default]
    Unknown,
}

// ==================== EIM 配置读取 ====================

/// EIM 安装配置（从 eim_idf.json 读取）
#[derive(Debug, Clone, Deserialize)]
struct EimIdfConfig {
    #[serde(rename = "idfInstalled")]
    idf_installed: Vec<EimIdfInstalled>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EimIdfInstalled {
    #[allow(dead_code)]
    pub name: String,
    pub path: String,
    #[serde(rename = "idfToolsPath")]
    pub idf_tools_path: String,
    pub python: String,
    #[allow(dead_code)]
    #[serde(rename = "activationScript")]
    pub activation_script: String,
}

/// 扫描目录下所有子目录（一级），返回完整路径
fn scan_tool_dirs(tools_path: &str, prefix: &str) -> Vec<PathBuf> {
    let base = Path::new(tools_path).join(prefix);
    if !base.exists() {
        return vec![];
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

/// 自动构建 EIM 工具链 PATH 条目
pub(crate) fn build_eim_path_entries(tools_path: &str) -> Vec<String> {
    let mut entries = Vec::new();

    // cmake
    for dir in scan_tool_dirs(tools_path, "cmake") {
        entries.push(dir.join("bin").to_string_lossy().to_string());
    }
    // ninja
    for dir in scan_tool_dirs(tools_path, "ninja") {
        entries.push(dir.to_string_lossy().to_string());
    }
    // ccache
    for dir in scan_tool_dirs(tools_path, "ccache") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            entries.push(sub.to_string_lossy().to_string());
        }
    }
    // dfu-util
    for dir in scan_tool_dirs(tools_path, "dfu-util") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            entries.push(sub.to_string_lossy().to_string());
        }
    }
    // xtensa-esp-elf (编译器)
    for dir in scan_tool_dirs(tools_path, "xtensa-esp-elf") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            entries.push(sub.join("bin").to_string_lossy().to_string());
            // 有的版本有两层 bin
            let inner = sub.join("xtensa-esp-elf").join("bin");
            if inner.exists() {
                entries.push(inner.to_string_lossy().to_string());
            }
        }
    }
    // riscv32-esp-elf
    for dir in scan_tool_dirs(tools_path, "riscv32-esp-elf") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            entries.push(sub.join("bin").to_string_lossy().to_string());
            let inner = sub.join("riscv32-esp-elf").join("bin");
            if inner.exists() {
                entries.push(inner.to_string_lossy().to_string());
            }
        }
    }
    // esp32ulp-elf
    for dir in scan_tool_dirs(tools_path, "esp32ulp-elf") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            entries.push(sub.join("bin").to_string_lossy().to_string());
            let inner = sub.join("esp32ulp-elf").join("bin");
            if inner.exists() {
                entries.push(inner.to_string_lossy().to_string());
            }
        }
    }
    // xtensa-esp-elf-gdb
    for dir in scan_tool_dirs(tools_path, "xtensa-esp-elf-gdb") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            entries.push(sub.join("bin").to_string_lossy().to_string());
        }
    }
    // openocd-esp32
    for dir in scan_tool_dirs(tools_path, "openocd-esp32") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            let ocd_bin = sub.join("openocd-esp32").join("bin");
            if ocd_bin.exists() {
                entries.push(ocd_bin.to_string_lossy().to_string());
            }
        }
    }
    // idf-exe
    for dir in scan_tool_dirs(tools_path, "idf-exe") {
        entries.push(dir.to_string_lossy().to_string());
    }
    // esp-rom-elfs
    for dir in scan_tool_dirs(tools_path, "esp-rom-elfs") {
        entries.push(dir.to_string_lossy().to_string());
    }
    // esp-clang
    for dir in scan_tool_dirs(tools_path, "esp-clang") {
        for sub in scan_tool_dirs(&dir.to_string_lossy(), "") {
            entries.push(sub.join("bin").to_string_lossy().to_string());
        }
    }

    entries
}

/// 自动发现 EIM 安装的 IDF 环境（读取 eim_idf.json）
fn detect_eim_setups() -> Vec<EimIdfInstalled> {
    let candidates = if cfg!(windows) {
        vec![
            r"C:\Espressif\tools\eim_idf.json".to_string(),
        ]
    } else {
        vec![
            shellexpand::tilde("~/.espressif/tools/eim_idf.json").to_string(),
        ]
    };

    for candidate in candidates {
        let path = Path::new(&candidate);
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    match serde_json::from_str::<EimIdfConfig>(&content) {
                        Ok(config) => {
                            info!("Found EIM IDF config: {} setups", config.idf_installed.len());
                            return config.idf_installed;
                        }
                        Err(e) => {
                            warn!("Failed to parse eim_idf.json: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read eim_idf.json: {}", e);
                }
            }
        }
    }
    vec![]
}

/// 为给定 idf_path 查找匹配的 EIM 配置
pub(crate) fn find_eim_setup(idf_path: &str) -> Option<EimIdfInstalled> {
    // 规范化路径用于比较
    let normalized = Path::new(idf_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(idf_path));

    let setups = detect_eim_setups();
    info!("find_eim_setup: looking for idf_path='{}', normalized='{}', {} setups available", idf_path, normalized.display(), setups.len());

    for setup in &setups {
        let setup_path = Path::new(&setup.path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&setup.path));
        if normalized == setup_path {
            info!("find_eim_setup: matched '{}' by canonicalized path, python={}", setup.name, setup.python);
            return Some(setup.clone());
        }
    }

    // 也尝试直接比较字符串
    for setup in &setups {
        if setup.path == idf_path {
            info!("find_eim_setup: matched '{}' by string, python={}", setup.name, setup.python);
            return Some(setup.clone());
        }
    }

    // 也尝试大小写不敏感比较（Windows）
    if cfg!(windows) {
        let idf_lower = idf_path.to_ascii_lowercase();
        for setup in &setups {
            let setup_lower = setup.path.to_ascii_lowercase();
            if idf_lower == setup_lower {
                info!("find_eim_setup: matched '{}' by case-insensitive, python={}", setup.name, setup.python);
                return Some(setup.clone());
            }
        }
    }

    warn!("find_eim_setup: no match for '{}', falling back to export.bat", idf_path);
    None
}

/// 公开版 EIM 查找（供 serial 模块等外部使用）
pub fn find_eim_setup_public(idf_path: &str) -> Option<EimIdfInstalled> {
    find_eim_setup(idf_path)
}

/// 查找 esptool.py 路径（参考官方扩展 esptool 调用方式）
///
/// 在 IDF 的 components/esptool_py/esptool/ 目录下查找
pub fn find_esptool_py(idf_path: &str) -> Option<PathBuf> {
    let idf = Path::new(idf_path);
    let candidates = [
        idf.join("components").join("esptool_py").join("esptool").join("esptool.py"),
        idf.join("tools").join("esptool.py"),
        idf.join("esptool.py"),
    ];
    for c in &candidates {
        if c.exists() {
            info!("Found esptool.py at: {}", c.display());
            return Some(c.clone());
        }
    }
    warn!("esptool.py not found under {}", idf_path);
    None
}

/// 从 IDF 的 tools/idf_py_actions/constants.py 解析 SUPPORTED_TARGETS 和 PREVIEW_TARGETS
/// 参考官方 getTargets.ts 实现
pub fn parse_supported_targets(idf_path: &str) -> Vec<ChipTargetInfo> {
    let constants_file = Path::new(idf_path)
        .join("tools")
        .join("idf_py_actions")
        .join("constants.py");

    let content = match std::fs::read_to_string(&constants_file) {
        Ok(c) => c,
        Err(e) => {
            warn!("Cannot read {}: {}", constants_file.display(), e);
            return get_fallback_targets();
        }
    };

    let mut result = Vec::new();
    result.extend(parse_target_list(&content, "SUPPORTED_TARGETS", false));
    result.extend(parse_target_list(&content, "PREVIEW_TARGETS", true));

    if result.is_empty() {
        warn!("No targets parsed from constants.py, using fallback");
        return get_fallback_targets();
    }
    result
}

/// 解析 Python 数组中的字符串元素
fn parse_target_list(content: &str, var_name: &str, is_preview: bool) -> Vec<ChipTargetInfo> {
    // 多行模式匹配: SUPPORTED_TARGETS = ['esp32', 'esp32s2', ...]
    let pattern = format!(r"{}\s*=\s*\[", var_name);
    let start_idx = match content.find(&pattern) {
        Some(pos) => {
            // 找到 = [ 后的位置
            content[pos..].find('[').map(|i| pos + i).unwrap_or(pos)
        }
        None => return vec![],
    };

    // 从 [ 开始扫描，找到匹配的 ]
    let slice = &content[start_idx..];
    let mut depth = 0;
    let mut end_idx = 0;
    for (i, ch) in slice.char_indices() {
        if ch == '[' { depth += 1; }
        else if ch == ']' { depth -= 1; if depth == 0 { end_idx = i; break; } }
    }

    let array_content = if end_idx > 0 { &slice[1..end_idx] } else { return vec![] };

    // 提取引号内的字符串
    let mut targets = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut current = String::new();

    for ch in array_content.chars() {
        match ch {
            '\'' if !in_double => {
                if in_single {
                    if !current.is_empty() {
                        let label = make_chip_label(&current);
                        targets.push(ChipTargetInfo {
                            target: current.clone(),
                            label,
                            is_preview,
                            description: if is_preview { "(Preview)".into() } else { String::new() },
                        });
                    }
                    current.clear();
                    in_single = false;
                } else {
                    in_single = true;
                }
            }
            '"' if !in_single => {
                if in_double {
                    if !current.is_empty() {
                        let label = make_chip_label(&current);
                        targets.push(ChipTargetInfo {
                            target: current.clone(),
                            label,
                            is_preview,
                            description: if is_preview { "(Preview)".into() } else { String::new() },
                        });
                    }
                    current.clear();
                    in_double = false;
                } else {
                    in_double = true;
                }
            }
            _ if in_single || in_double => {
                current.push(ch);
            }
            _ => {}
        }
    }
    targets
}

fn make_chip_label(target: &str) -> String {
    // esp32s3 -> ESP32-S3, esp32c6 -> ESP32-C6, esp32 -> ESP32
    let upper = target.to_uppercase();
    // Insert dash before S/H/C/P number
    upper
        .replace("S3", "-S3")
        .replace("S2", "-S2")
        .replace("C2", "-C2")
        .replace("C3", "-C3")
        .replace("C5", "-C5")
        .replace("C6", "-C6")
        .replace("C61", "-C61")
        .replace("H2", "-H2")
        .replace("H21", "-H21")
        .replace("H4", "-H4")
        .replace("P4", "-P4")
}

fn get_fallback_targets() -> Vec<ChipTargetInfo> {
    vec![
        ChipTargetInfo { target: "esp32".into(), label: "ESP32".into(), is_preview: false, description: String::new() },
        ChipTargetInfo { target: "esp32s2".into(), label: "ESP32-S2".into(), is_preview: false, description: String::new() },
        ChipTargetInfo { target: "esp32s3".into(), label: "ESP32-S3".into(), is_preview: false, description: String::new() },
        ChipTargetInfo { target: "esp32c2".into(), label: "ESP32-C2".into(), is_preview: false, description: String::new() },
        ChipTargetInfo { target: "esp32c3".into(), label: "ESP32-C3".into(), is_preview: false, description: String::new() }, 
        ChipTargetInfo { target: "esp32c5".into(), label: "ESP32-C5".into(), is_preview: false, description: String::new() },
        ChipTargetInfo { target: "esp32c6".into(), label: "ESP32-C6".into(), is_preview: false, description: String::new() },
        ChipTargetInfo { target: "esp32h2".into(), label: "ESP32-H2".into(), is_preview: false, description: String::new() },    
        ChipTargetInfo { target: "esp32p4".into(), label: "ESP32-P4".into(), is_preview: false, description: String::new() },
        ChipTargetInfo { target: "esp32s31".into(), label: "ESP32-S31".into(), is_preview: false, description: String::new() },
        
    ]
}

/// 芯片目标信息（对应官方 IdfTarget）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChipTargetInfo {
    pub target: String,
    pub label: String,
    pub is_preview: bool,
    #[serde(default)]
    pub description: String,
}

// ==================== IDF 检测 ====================

/// 自动检测 ESP-IDF
pub fn detect_idf() -> Option<IDFEnvironment> {
    // 优先级 0：EIM 安装（最可靠，有完整配置）
    let eim_setups = detect_eim_setups();
    for setup in &eim_setups {
        let path = Path::new(&setup.path);
        if path.exists() && path.join("tools").exists() {
            info!("Detected ESP-IDF from EIM: {} at {}", setup.name, setup.path);
            return Some(IDFEnvironment {
                idf_path: setup.path.clone(),
                version: setup.name.clone(),
                tools_path: setup.idf_tools_path.clone(),
                python_path: Some(setup.python.clone()),
                source: DetectionSource::EimDiscovered,
            });
        }
    }

    // 优先级1：检查 IDF_PATH 环境变量
    if let Ok(idf_path) = std::env::var("IDF_PATH") {
        let path = Path::new(&idf_path);
        if path.exists() && path.join("tools").exists() {
            info!("Detected ESP-IDF from IDF_PATH: {}", idf_path);
            return Some(IDFEnvironment {
                idf_path: idf_path.clone(),
                version: get_idf_version(&idf_path),
                tools_path: path.join("tools").to_string_lossy().to_string(),
                python_path: None,
                source: DetectionSource::EnvVariable,
            });
        }
    }

    // 优先级2：检查常见安装位置
    let candidates = get_default_candidates();
    for path_str in candidates {
        let expanded = shellexpand::tilde(&path_str);
        let path = Path::new(&*expanded);
        if path.exists() && path.join("tools").exists() && path.join("idf.py").exists() {
            info!("Detected ESP-IDF at: {}", expanded);
            return Some(IDFEnvironment {
                idf_path: expanded.to_string(),
                version: get_idf_version(&expanded),
                tools_path: path.join("tools").to_string_lossy().to_string(),
                python_path: None,
                source: DetectionSource::AutoDetected,
            });
        }
    }

    None
}

/// 获取默认候选位置
fn get_default_candidates() -> Vec<String> {
    let mut candidates = Vec::new();

    if cfg!(windows) {
        candidates.push(r"C:\Espressif\frameworks\esp-idf".to_string());
        candidates.push(r"C:\esp-idf".to_string());
        // EIM 常见位置
        if let Ok(home) = std::env::var("USERPROFILE") {
            candidates.push(format!(r"{}\.espressif\esp-idf", home));
        }
    } else {
        candidates.push("~/esp/esp-idf".to_string());
        candidates.push("~/.espressif/esp-idf".to_string());
        candidates.push("/opt/esp-idf".to_string());
        candidates.push("/usr/local/esp-idf".to_string());
    }

    candidates
}

/// 获取 ESP-IDF 版本
///
/// 优先级: version.txt → 路径名中的版本号 → git describe（catch_unwind 防护）
pub fn get_idf_version(idf_path: &str) -> String {
    // 1. 从 version.txt 读取
    let version_file = Path::new(idf_path).join("version.txt");
    if version_file.exists() {
        if let Ok(content) = std::fs::read_to_string(version_file) {
            return content.trim().to_string();
        }
    }

    // 2. 从 IDF 路径中提取（如 v6.0/esp-idf → 6.0）
    if let Some(ver) = extract_version_from_path(idf_path) {
        return ver;
    }

    // 3. git describe（Command::output() 在 Windows 终端可能 panic，用 catch_unwind 防护）
    if let Ok(Ok(output)) = std::panic::catch_unwind(|| {
        let mut cmd = Command::new("git");
        cmd.args(["-C", idf_path, "describe", "--tags", "--always"]);
        #[cfg(windows)]
        { cmd.creation_flags(0x08000000); }
        cmd.output()
    }) {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).trim().to_string();
            }
    }

    "unknown".to_string()
}

/// 从 IDF 路径中提取版本号
/// 例如: E:\espeim\.espressif\v6.0\esp-idf → "6.0"
fn extract_version_from_path(idf_path: &str) -> Option<String> {
    let path = Path::new(idf_path);
    // 查找形如 vX.Y 的父目录名
    if let Some(parent) = path.parent() {
        let folder = parent.file_name()?.to_string_lossy();
        if folder.starts_with('v') || folder.starts_with('V') {
            let ver = &folder[1..]; // 去掉前缀 'v'
            if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some(ver.to_string());
            }
        }
    }
    // 如果没找到，检查路径本身是否包含 vX.Y
    if let Some(folder) = path.file_name() {
        let folder = folder.to_string_lossy();
        if folder.starts_with('v') || folder.starts_with('V') {
            let ver = &folder[1..];
            if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some(ver.to_string());
            }
        }
    }
    None
}

/// 验证给定路径是否是有效的 ESP-IDF 目录
pub fn validate_idf_path(path: &str) -> Result<IDFEnvironment, String> {
    let sanitized = sanitize_idf_path(path);
    let idf_path = Path::new(sanitized.as_str());
    if !idf_path.exists() {
        return Err("Path does not exist".to_string());
    }
    if !idf_path.join("tools").exists() {
        return Err("Not an ESP-IDF directory (missing 'tools' subdir)".to_string());
    }
    let has_idf_py = idf_path.join("idf.py").exists() || idf_path.join("tools").join("idf.py").exists();
    if !has_idf_py {
        return Err("Not an ESP-IDF directory (missing 'idf.py')".to_string());
    }

    let mut python_path: Option<String> = None;
    let eim_setup = find_eim_setup(&sanitized);
    if let Some(ref setup) = eim_setup {
        python_path = Some(setup.python.clone());
        info!("Python from EIM: {}", setup.python);
    }

    if python_path.is_none() {
        let tools_path = idf_path.join("tools");
        let venv_candidates = if cfg!(windows) {
            vec![
                tools_path.join("python").join("Scripts").join("python.exe"),
                tools_path.join("python").join("python.exe"),
            ]
        } else {
            vec![
                tools_path.join("python").join("bin").join("python3"),
                tools_path.join("python").join("bin").join("python"),
            ]
        };
        for candidate in &venv_candidates {
            if candidate.exists() {
                python_path = Some(candidate.to_string_lossy().to_string());
                info!("Python from venv: {}", candidate.display());
                break;
            }
        }
    }

    if python_path.is_none() {
        let eim_setups = detect_eim_setups();
        for setup in &eim_setups {
            let setup_path = Path::new(&setup.path).canonicalize().unwrap_or_else(|_| PathBuf::from(&setup.path));
            let current = idf_path.canonicalize().unwrap_or_else(|_| idf_path.to_path_buf());
            if setup_path == current {
                python_path = Some(setup.python.clone());
                info!("Python from EIM (path match): {}", setup.python);
                break;
            }
        }
    }

    let tools_path = Path::new(sanitized.as_str()).join("tools").to_string_lossy().to_string();

    Ok(IDFEnvironment {
        idf_path: sanitized,
        version: get_idf_version(path),
        tools_path,
        python_path,
        source: DetectionSource::UserConfigured,
    })
}

/// 清洗 IDF 路径：如果路径末尾是 tools 子目录，自动回退到父目录
pub(crate) fn sanitize_idf_path(idf_path: &str) -> String {
    let path = Path::new(idf_path);
    if path.file_name().map(|n| n == "tools").unwrap_or(false) {
        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            let has_export = if cfg!(windows) {
                parent.join("export.bat").exists()
            } else {
                parent.join("export.sh").exists()
            };
            if has_export {
                info!("IDF path ends with 'tools', auto-correcting to: {}", parent_str);
                return parent_str;
            }
        }

        let normalized = idf_path.trim_end_matches('\\').trim_end_matches('/').to_ascii_lowercase();
        let setups = detect_eim_setups();
        for setup in &setups {
            let tools_normalized = setup.idf_tools_path.trim_end_matches('\\').trim_end_matches('/').to_ascii_lowercase();
            if normalized == tools_normalized {
                info!("IDF path matches EIM tools path, auto-correcting to: {}", setup.path);
                return setup.path.clone();
            }
        }
    }
    idf_path.to_string()
}

/// 查找 idf.py 路径，优先在根目录查找，也尝试 tools 子目录
pub(crate) fn find_idf_py(idf_path: &str) -> Option<std::path::PathBuf> {
    let path = Path::new(idf_path);
    let root_py = path.join("idf.py");
    if root_py.exists() {
        return Some(root_py);
    }
    let tools_py = path.join("tools").join("idf.py");
    if tools_py.exists() {
        return Some(tools_py);
    }

    let setups = detect_eim_setups();
    let normalized = idf_path.trim_end_matches('\\').trim_end_matches('/').to_ascii_lowercase();
    for setup in &setups {
        let tools_normalized = setup.idf_tools_path.trim_end_matches('\\').trim_end_matches('/').to_ascii_lowercase();
        if normalized == tools_normalized || normalized.starts_with(&tools_normalized) {
            let eim_idf = Path::new(&setup.path);
            let eim_root = eim_idf.join("idf.py");
            if eim_root.exists() {
                info!("Found idf.py via EIM config: {}", eim_root.display());
                return Some(eim_root);
            }
            let eim_tools = eim_idf.join("tools").join("idf.py");
            if eim_tools.exists() {
                info!("Found idf.py via EIM config: {}", eim_tools.display());
                return Some(eim_tools);
            }
        }
    }

    None
}

// ==================== idf.py 命令执行 ====================

/// 获取 ESP-IDF 版本（用于设置环境变量 ESP_IDF_VERSION）
/// 去除前导 'v'，确保返回如 "6.0" 而非 "v6.0"
pub(crate) fn get_idf_version_for_env(idf_path: &str) -> String {
    let version = get_idf_version(idf_path);
    version.strip_prefix('v').unwrap_or(&version).to_string()
}

/// 从项目 sdkconfig 中读取 CONFIG_IDF_TARGET，用于设置 ESP_ROM_ELF_DIR
fn detect_target_from_project(project_path: &str) -> Option<String> {
    let sdkconfig = Path::new(project_path).join("sdkconfig");
    if !sdkconfig.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&sdkconfig).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("CONFIG_IDF_TARGET=") {
            return Some(val.trim_matches('"').to_string());
        }
    }
    None
}

/// 执行 idf.py 命令（通过虚拟环境 Python 或 export.bat）
///
/// 优先使用 EIM/VSCode 扩展安装的虚拟环境 Python 直接调用 idf.py，
/// 如果找不到 EIM 配置则回退到 export.bat 方式。
pub fn run_idf_command(project_path: &str, idf_path: &str, args: &[&str]) -> Result<String, String> {
    let idf_path = sanitize_idf_path(idf_path);
    let idf_path = idf_path.as_str();
    info!("Running idf.py {:?} in {}", args, project_path);

    let idf_py = find_idf_py(idf_path)
        .ok_or_else(|| {
            let root_py = Path::new(idf_path).join("idf.py");
            format!("idf.py not found at {} or {}", root_py.display(), Path::new(idf_path).join("tools").join("idf.py").display())
        })?;

    let args_str = args.iter()
        .map(|a| format!("\"{}\"", a))
        .collect::<Vec<_>>()
        .join(" ");

    // 尝试 EIM 方式：用虚拟环境 Python 直接执行
    if let Some(eim_setup) = find_eim_setup(idf_path) {
        let python_path = &eim_setup.python;
        let tools_path = &eim_setup.idf_tools_path;
        info!("Using EIM venv Python: {} with tools at {}", python_path, tools_path);

        return run_with_eim_python(project_path, python_path, &idf_py, idf_path, tools_path, args);
    }

    // 回退：export.bat 方式
    run_with_export_bat(project_path, idf_path, &idf_py, &args_str)
}

/// 使用 EIM 虚拟环境 Python 执行 idf.py
fn run_with_eim_python(
    project_path: &str,
    python_path: &str,
    idf_py: &Path,
    idf_path: &str,
    tools_path: &str,
    args: &[&str],
) -> Result<String, String> {
    let python_path = python_path.replace('/', "\\");

    // 验证 Python 路径存在
    if !Path::new(&python_path).exists() {
        return Err(format!(
            "EIM Python 路径不存在: {}，请重新安装 ESP-IDF 或检查 EIM 配置",
            python_path
        ));
    }

    // 确保工作目录存在（idf.py create-project 会创建项目子目录）
    if !Path::new(project_path).exists() {
        std::fs::create_dir_all(project_path).map_err(|e| format!(
            "无法创建工作目录 {}: {}",
            project_path, e
        ))?;
    }

    // 构建 PATH：工具链路径 + 原 PATH
    let eim_path_entries = build_eim_path_entries(tools_path);
    let python_scripts = Path::new(&python_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let system_path = std::env::var("PATH").unwrap_or_default();

    let new_path = if eim_path_entries.is_empty() {
        format!("{};{}", python_scripts, system_path)
    } else {
        format!("{};{};{}", eim_path_entries.join(";"), python_scripts, system_path)
    };

    // 设置 IDF_PYTHON_ENV_PATH（python 的 venv 根目录）
    let idf_python_env_path = Path::new(&python_path)
        .parent()  // Scripts
        .and_then(|p| p.parent())  // venv
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // idf.py 需要找到 tools/ 下的模块（如 python_version_checker）
    let idf_tools_dir = format!("{}\\tools", idf_path);

    info!("EIM env: IDF_PATH={}, IDF_TOOLS_PATH={}, IDF_PYTHON_ENV_PATH={}, ESP_IDF_VERSION={}",
        idf_path, tools_path, idf_python_env_path, get_idf_version_for_env(idf_path));

    let output = {
        let mut cmd = Command::new(&python_path);
        cmd.arg(idf_py)
            .args(args)
            .current_dir(project_path)
            .env("IDF_PATH", idf_path)
            .env("IDF_TOOLS_PATH", tools_path)
            .env("IDF_PYTHON_ENV_PATH", &idf_python_env_path)
            .env("ESP_IDF_VERSION", get_idf_version_for_env(idf_path))
            .env("IDF_COMPONENT_MANAGER", "1")
            .env("PATH", &new_path)
            .env("PYTHONPATH", format!("{};{}", &idf_tools_dir, std::env::var("PYTHONPATH").unwrap_or_default()))
            .env("OPENOCD_SCRIPTS", format!("{}\\openocd-esp32", tools_path))
            .env("ESP_ROM_ELF_DIR", format!("{}\\components\\esp_rom\\{}", idf_path,
                detect_target_from_project(project_path).unwrap_or_else(|| "esp32".to_string())))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)] { cmd.creation_flags(0x08000000); }
        cmd.spawn()
    }
    .and_then(|child| child.wait_with_output())
        .map_err(|e| format!("Failed to execute idf.py via EIM python: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("{}\n{}", stdout, stderr))
    }
}

/// 使用 export.bat 方式执行 idf.py（回退方案）
fn run_with_export_bat(
    project_path: &str,
    idf_path: &str,
    idf_py: &Path,
    args_str: &str,
) -> Result<String, String> {
    let output = if cfg!(windows) {
        let export_bat = Path::new(idf_path).join("export.bat");
        if !export_bat.exists() {
            return Err(format!("export.bat not found at {}", export_bat.display()));
        }
        let cmd_str = format!(
            "call {} >nul && set ESP_IDF_VERSION={} && python {} {}",
            export_bat.display(),
            get_idf_version_for_env(idf_path),
            idf_py.display(),
            args_str
        );
        info!("Executing (export.bat): cmd /C {}", cmd_str);
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &cmd_str])
            .current_dir(project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)] { cmd.creation_flags(0x08000000); }
        cmd.spawn()
            .and_then(|child| child.wait_with_output())
            .map_err(|e| format!("Failed to execute idf.py via cmd: {}", e))?
    } else {
        let export_sh = Path::new(idf_path).join("export.sh");
        if !export_sh.exists() {
            return Err(format!("export.sh not found at {}", export_sh.display()));
        }
        let cmd_str = format!(
            "source \"{}\" >/dev/null 2>&1 && export ESP_IDF_VERSION={} && python \"{}\" {}",
            export_sh.display(),
            get_idf_version_for_env(idf_path),
            idf_py.display(),
            args_str
        );
        info!("Executing (export.sh): bash -c {}", cmd_str);
        Command::new("bash")
            .args(["-c", &cmd_str])
            .current_dir(project_path)
            .output()
            .map_err(|e| format!("Failed to execute idf.py via bash: {}", e))?
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("{}\n{}", stdout, stderr))
    }
}

// ==================== 流式执行（异步，不阻塞 UI） ====================

/// 构建输出事件负载
#[derive(Debug, Clone, Serialize)]
pub struct BuildOutputPayload {
    pub line: String,
    pub is_stderr: bool,
}

/// 构建完成事件负载
#[derive(Debug, Clone, Serialize)]
pub struct BuildDonePayload {
    pub success: bool,
    pub errors: Vec<crate::commands::build::CompileError>,
}

/// 流式执行 idf.py 命令，将输出实时推送到前端
pub fn run_idf_command_streaming(
    app: &tauri::AppHandle,
    project_path: &str,
    idf_path: &str,
    args: &[&str],
) -> Result<String, String> {
    let idf_path_sanitized = sanitize_idf_path(idf_path);
    let idf_path_str = idf_path_sanitized.as_str();
    info!("Streaming idf.py {:?} in {}", args, project_path);

    let idf_py = find_idf_py(idf_path_str)
        .ok_or_else(|| {
            let root_py = Path::new(idf_path_str).join("idf.py");
            format!("idf.py not found at {} or {}", root_py.display(), Path::new(idf_path_str).join("tools").join("idf.py").display())
        })?;
    let idf_py = Arc::new(idf_py);

    let app_handle = app.clone();
    let project_path = project_path.to_string();
    let args_vec: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    // 尝试 EIM 方式
    if let Some(eim_setup) = find_eim_setup(idf_path_str) {
        let python_path = eim_setup.python.replace('/', "\\");
        let tools_path = eim_setup.idf_tools_path.clone();

        let eim_path_entries = build_eim_path_entries(&tools_path);
        let python_scripts = Path::new(&python_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let system_path = std::env::var("PATH").unwrap_or_default();
        let new_path = if eim_path_entries.is_empty() {
            format!("{};{}", python_scripts, system_path)
        } else {
            format!("{};{};{}", eim_path_entries.join(";"), python_scripts, system_path)
        };
        let idf_python_env_path = Path::new(&python_path)
            .parent().and_then(|p| p.parent())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let idf_path = idf_path_str.to_string();
        let esp_idf_version = get_idf_version_for_env(&idf_path);
        let idf_tools_dir = format!("{}\\tools", idf_path_str);
        info!("Streaming EIM: python={}, idf_path={}, tools={}, version={}", python_path, idf_path, tools_path, esp_idf_version);

        std::thread::spawn(move || {
            let mut cmd = Command::new(&python_path);
            cmd.arg(idf_py.as_os_str())
               .args(&args_vec)
               .current_dir(&project_path)
               .env("IDF_PATH", &idf_path)
               .env("IDF_TOOLS_PATH", &tools_path)
               .env("IDF_PYTHON_ENV_PATH", &idf_python_env_path)
               .env("ESP_IDF_VERSION", &esp_idf_version)
               .env("IDF_COMPONENT_MANAGER", "1")
               .env("PATH", &new_path)
               .env("PYTHONPATH", format!("{};{}", idf_tools_dir, std::env::var("PYTHONPATH").unwrap_or_default()))
               .env("OPENOCD_SCRIPTS", format!("{}\\openocd-esp32", tools_path))
               .env("ESP_ROM_ELF_DIR", format!("{}\\components\\esp_rom\\{}", idf_path,
                   detect_target_from_project(&project_path).unwrap_or_else(|| "esp32".to_string())))
               .stdout(Stdio::piped())
               .stderr(Stdio::piped());
            #[cfg(windows)] { cmd.creation_flags(0x08000000); }

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = app_handle.emit("build-output", BuildOutputPayload {
                        line: format!("Failed to spawn idf.py: {}", e),
                        is_stderr: true,
                    });
                    let _ = app_handle.emit("build-done", BuildDonePayload {
                        success: false, errors: vec![],
                    });
                    return;
                }
            };

            if let Some(stdout) = child.stdout.take() {
                let app = app_handle.clone();
                std::thread::spawn(move || {
                    for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                            let _ = app.emit("build-output", BuildOutputPayload { line: l, is_stderr: false });
                    }
                });
            }
            if let Some(stderr) = child.stderr.take() {
                let app = app_handle.clone();
                std::thread::spawn(move || {
                    for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                            let _ = app.emit("build-output", BuildOutputPayload { line: l, is_stderr: true });
                    }
                });
            }

            let status = child.wait().unwrap_or_default();
            let _ = app_handle.emit("build-done", BuildDonePayload { success: status.success(), errors: vec![] });
        });

        return Ok("Build started (EIM streaming)".into());
    }

    // 回退：export.bat 方式
    if cfg!(windows) {
        let export_bat = Path::new(idf_path_str).join("export.bat");
        if !export_bat.exists() {
            return Err(format!("export.bat not found at {}", export_bat.display()));
        }
        let export_bat = Arc::new(export_bat);
        let esp_idf_version_fallback = get_idf_version_for_env(idf_path_str);

        std::thread::spawn(move || {
            let cmd_str = format!(
                "call {} >nul && set ESP_IDF_VERSION={} && python {} {}",
                export_bat.display(),
                esp_idf_version_fallback,
                idf_py.display(),
                args_vec.iter().map(|a| format!("\"{}\"", a)).collect::<Vec<_>>().join(" ")
            );
            info!("Streaming cmd: {}", cmd_str);

            let mut cmd = Command::new("cmd");
            cmd.args(["/C", &cmd_str])
                .current_dir(&project_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(windows)] { cmd.creation_flags(0x08000000); }
            let mut child = match cmd.spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = app_handle.emit("build-output", BuildOutputPayload {
                        line: format!("Failed to spawn cmd: {}", e), is_stderr: true,
                    });
                    let _ = app_handle.emit("build-done", BuildDonePayload { success: false, errors: vec![] });
                    return;
                }
            };

            if let Some(stdout) = child.stdout.take() {
                let app = app_handle.clone();
                std::thread::spawn(move || {
                    for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                            let _ = app.emit("build-output", BuildOutputPayload { line: l, is_stderr: false });
                    }
                });
            }
            if let Some(stderr) = child.stderr.take() {
                let app = app_handle.clone();
                std::thread::spawn(move || {
                    for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                            let _ = app.emit("build-output", BuildOutputPayload { line: l, is_stderr: true });
                    }
                });
            }

            let status = child.wait().unwrap_or_default();
            let _ = app_handle.emit("build-done", BuildDonePayload { success: status.success(), errors: vec![] });
        });

        return Ok("Build started (cmd streaming)".into());
    }

    // Linux/macOS
    let export_sh = Path::new(idf_path_str).join("export.sh");
    if !export_sh.exists() {
        return Err(format!("export.sh not found at {}", export_sh.display()));
    }
    let export_sh = Arc::new(export_sh);
    let esp_idf_version_linux = get_idf_version_for_env(idf_path_str);

    std::thread::spawn(move || {
        let cmd_str = format!(
            "source \"{}\" >/dev/null 2>&1 && export ESP_IDF_VERSION={} && export IDF_COMPONENT_MANAGER=1 && python \"{}\" {}",
            export_sh.display(),
            esp_idf_version_linux,
            idf_py.display(),
            args_vec.iter().map(|a| format!("\"{}\"", a)).collect::<Vec<_>>().join(" ")
        );

        let mut child = match Command::new("bash")
            .args(["-c", &cmd_str])
            .current_dir(&project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = app_handle.emit("build-output", BuildOutputPayload {
                    line: format!("Failed to spawn bash: {}", e), is_stderr: true,
                });
                let _ = app_handle.emit("build-done", BuildDonePayload { success: false, errors: vec![] });
                return;
            }
        };

        if let Some(stdout) = child.stdout.take() {
            let app = app_handle.clone();
            std::thread::spawn(move || {
                for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                        let _ = app.emit("build-output", BuildOutputPayload { line: l, is_stderr: false });
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let app = app_handle.clone();
            std::thread::spawn(move || {
                for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                        let _ = app.emit("build-output", BuildOutputPayload { line: l, is_stderr: true });
                }
            });
        }

        let status = child.wait().unwrap_or_default();
        let _ = app_handle.emit("build-done", BuildDonePayload { success: status.success(), errors: vec![] });
    });

    Ok("Build started (bash streaming)".into())
}

// ==================== 便捷函数 ====================

pub fn idf_create_project(parent_path: &str, project_name: &str, idf_path: &str) -> Result<(), String> {
    info!("Running idf.py create-project {} in {}", project_name, parent_path);
    run_idf_command(parent_path, idf_path, &["create-project", project_name])
        .map(|output| { info!("idf.py create-project output:\n{}", output); })
}

pub fn set_target(project_path: &str, idf_path: &str, target: &str) -> Result<(), String> {
    info!("Running idf.py set-target {} in {}", target, project_path);
    run_idf_command(project_path, idf_path, &["set-target", target])
        .map(|output| { info!("idf.py set-target output:\n{}", output); })
}

/// 设置目标芯片（流式输出，不阻塞 UI）
#[tauri::command]
pub fn idf_set_target(app: tauri::AppHandle, project_path: String, idf_path: String, target: String) -> Result<String, String> {
    // 归一化目标名称：ESP32-S3 → esp32s3（idf.py set-target 要求小写无连字符）
    let normalized = target.to_lowercase().replace('-', "");
    info!("Setting target (streaming): {} (from {}) in project {}", normalized, target, project_path);
    run_idf_command_streaming(&app, &project_path, &idf_path, &["set-target", &normalized])
}

pub fn build(project_path: &str, idf_path: &str) -> Result<String, String> {
    run_idf_command(project_path, idf_path, &["build"])
}

pub fn flash(project_path: &str, idf_path: &str, port: &str) -> Result<String, String> {
    run_idf_command(project_path, idf_path, &["-p", port, "flash"])
}

pub fn monitor(project_path: &str, idf_path: &str, port: &str, baudrate: u32) -> Result<String, String> {
    run_idf_command(project_path, idf_path, &["-p", port, "-b", &baudrate.to_string(), "monitor"])
}

pub fn menuconfig(project_path: &str, idf_path: &str) -> Result<String, String> {
    info!("Opening menuconfig for {}", project_path);
    run_idf_command_foreground(project_path, idf_path, &["menuconfig"])
}

pub fn clean(project_path: &str, idf_path: &str) -> Result<String, String> {
    info!("Cleaning project: {}", project_path);
    run_idf_command(project_path, idf_path, &["clean"])
}

pub fn fullclean(project_path: &str, idf_path: &str) -> Result<String, String> {
    info!("Full cleaning project: {}", project_path);
    run_idf_command(project_path, idf_path, &["fullclean"])
}

pub fn size(project_path: &str, idf_path: &str) -> Result<String, String> {
    info!("Analyzing firmware size for {}", project_path);
    // Try JSON format first (--json size-components), fallback to text
    let json_result = run_idf_command(project_path, idf_path, &["--json", "size-components"]);
    match json_result {
        Ok(output) if !output.trim().is_empty() && serde_json::from_str::<serde_json::Value>(&output).is_ok() => {
                return Ok(output);
        }
        _ => {}
    }
    // Fallback to plain text output
    run_idf_command(project_path, idf_path, &["size-components"])
}

/// Get firmware size as structured data for frontend visualization
#[tauri::command]
pub async fn idf_size_json(project_path: String, idf_path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let output = size(&project_path, &idf_path)?;
        // Try parsing as JSON first, if it's the text format, convert to structured
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output) {
            return Ok(json);
        }
        // Parse text format: lines like "component_name  size_in_bytes"
        let mut components = Vec::new();
        for line in output.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(bytes) = parts[parts.len() - 1].parse::<u64>() {
                    components.push(serde_json::json!({
                        "name": parts[0],
                        "size_bytes": bytes,
                        "size_kb": bytes as f64 / 1024.0,
                    }));
                }
            }
        }
        Ok(serde_json::json!({
            "raw_output": output,
            "components": components,
            "format": "text"
        }))
    }).await.map_err(|e| e.to_string())?
}

pub fn erase_flash(project_path: &str, idf_path: &str, port: &str) -> Result<String, String> {
    info!("Erasing flash on port {} for {}", port, project_path);
    run_idf_command(project_path, idf_path, &["-p", port, "erase-flash"])
}

pub fn build_flash_monitor(project_path: &str, idf_path: &str, port: &str) -> Result<String, String> {
    info!("Build + Flash + Monitor for {} on {}", project_path, port);
    run_idf_command_foreground(project_path, idf_path, &["-p", port, "build", "flash", "monitor"])
}

/// 在新终端窗口中运行 idf.py 命令（用于交互式命令如 menuconfig、monitor）
fn run_idf_command_foreground(project_path: &str, idf_path: &str, args: &[&str]) -> Result<String, String> {
    let idf_path = sanitize_idf_path(idf_path);
    let idf_path = idf_path.as_str();

    let idf_py = find_idf_py(idf_path)
        .ok_or_else(|| {
            let root_py = Path::new(idf_path).join("idf.py");
            format!("idf.py not found at {} or {}", root_py.display(), Path::new(idf_path).join("tools").join("idf.py").display())
        })?;

    let args_str = args.iter()
        .map(|a| format!("\"{}\"", a))
        .collect::<Vec<_>>()
        .join(" ");

    // EIM 方式：用虚拟环境 Python + 设置环境变量的 cmd 窗口
    if let Some(eim_setup) = find_eim_setup(idf_path) {
        let python_path = eim_setup.python.replace('/', "\\");
        let tools_path = &eim_setup.idf_tools_path;
        let eim_path_entries = build_eim_path_entries(tools_path);
        let python_scripts = Path::new(&python_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let idf_python_env_path = Path::new(&python_path)
            .parent().and_then(|p| p.parent())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let path_str = if eim_path_entries.is_empty() {
            format!("{};%PATH%", python_scripts)
        } else {
            format!("{};{};%PATH%", eim_path_entries.join(";"), python_scripts)
        };

        // 在 cmd 窗口中先设置环境变量，再执行
        let esp_idf_ver = get_idf_version_for_env(idf_path);
        let cmd_str = format!(
            "set IDF_PATH={} && set IDF_TOOLS_PATH={} && set IDF_PYTHON_ENV_PATH={} && set ESP_IDF_VERSION={} && set PATH={} && python {} {} {} && echo. && echo Press any key to close... && pause >nul",
            idf_path, tools_path, idf_python_env_path, esp_idf_ver,
            path_str,
            python_path, idf_py.display(), args_str
        );
        info!("Spawning foreground EIM cmd: {}", cmd_str);
        Command::new("cmd")
            .args(["/C", "start", "ESP-IDF Command", "/K", &cmd_str])
            .current_dir(project_path)
            .spawn()
            .map_err(|e| format!("Failed to spawn terminal: {}", e))?;
    } else if cfg!(windows) {
        // 回退：export.bat
        let export_bat = Path::new(idf_path).join("export.bat");
        if !export_bat.exists() {
            return Err(format!("export.bat not found at {}", export_bat.display()));
        }
        let cmd_str = format!(
            "call {} && set ESP_IDF_VERSION={} && python {} {} && echo. && echo Press any key to close... && pause >nul",
            export_bat.display(), get_idf_version_for_env(idf_path), idf_py.display(), args_str
        );
        info!("Spawning foreground cmd: {}", cmd_str);
        Command::new("cmd")
            .args(["/C", "start", "ESP-IDF Command", "/K", &cmd_str])
            .current_dir(project_path)
            .spawn()
            .map_err(|e| format!("Failed to spawn terminal: {}", e))?;
    } else {
        let export_sh = Path::new(idf_path).join("export.sh");
        if !export_sh.exists() {
            return Err(format!("export.sh not found at {}", export_sh.display()));
        }
        let cmd_str = format!(
            "source \"{}\" && export ESP_IDF_VERSION={} && python \"{}\" {}; echo 'Press Enter to close...'; read",
            export_sh.display(), get_idf_version_for_env(idf_path), idf_py.display(), args_str
        );
        info!("Spawning foreground terminal: {}", cmd_str);
        Command::new("x-terminal-emulator")
            .args(["-e", &format!("bash -c '{}'", cmd_str)])
            .current_dir(project_path)
            .spawn()
            .map_err(|e| format!("Failed to spawn terminal: {}", e))?;
    }

    Ok("Command launched in external terminal".to_string())
}

// ============== Tauri Commands ==============

/// 检测 ESP-IDF
#[tauri::command]
pub fn idf_detect() -> Result<Option<IDFEnvironment>, String> {
    Ok(detect_idf())
}

/// 验证用户提供的 ESP-IDF 路径
#[tauri::command]
pub fn idf_validate_path(path: String) -> Result<IDFEnvironment, String> {
    validate_idf_path(&path)
}

/// 构建项目（流式输出，不阻塞 UI）
#[tauri::command]
pub fn idf_build(app: tauri::AppHandle, project_path: String, idf_path: String) -> Result<String, String> {
    info!("Building project (streaming): {} with ESP-IDF at {}", project_path, idf_path);
    run_idf_command_streaming(&app, &project_path, &idf_path, &["build"])
}

/// 构建项目（同步，返回完整输出 - 供 build_project 兼容）
pub fn build_sync(project_path: &str, idf_path: &str) -> Result<String, String> {
    let log_dir = Path::new(project_path).join("build").join("log");
    if log_dir.exists() {
        let _ = std::fs::remove_dir_all(&log_dir);
    }
    build(project_path, idf_path)
}

#[tauri::command]
pub fn idf_flash(app: tauri::AppHandle, project_path: String, idf_path: String, port: String) -> Result<String, String> {
    info!("Flashing project (streaming): {} to port: {} with ESP-IDF at {}", project_path, port, idf_path);
    run_idf_command_streaming(&app, &project_path, &idf_path, &["-p", &port, "flash"])
}

pub fn parse_compile_errors(output: &str) -> Vec<crate::commands::build::CompileError> {
    let mut errors = Vec::new();

    // Pattern 1: GCC/Clang format: file:line:col: error|warning: message
    let gcc_re = regex::Regex::new(r"([^:]+):(\d+):(\d+):\s+(error|warning):\s+(.+)").unwrap();

    // Pattern 2: CMake format: CMake Error/Warning at path:line (function):
    // Followed by indented message on subsequent lines
    let cmake_re = regex::Regex::new(r"CMake\s+(Error|Warning)\s+at\s+([^:]+):(\d+)\s*\(([^)]*)\)\s*:").unwrap();

    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        // Try GCC/Clang format
        if let Some(caps) = gcc_re.captures(line) {
            if caps.len() >= 6 {
                errors.push(crate::commands::build::CompileError {
                    file: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
                    line: caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
                    column: caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
                    error_type: caps.get(4).map(|m| m.as_str()).unwrap_or("error").to_string(),
                    message: caps.get(5).map(|m| m.as_str()).unwrap_or("").to_string(),
                });
            }
        }

        // Try CMake format
        if let Some(caps) = cmake_re.captures(line) {
            let error_type = caps.get(1).map(|m| m.as_str()).unwrap_or("Error").to_lowercase();
            let file = caps.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
            let line_num = caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);

            // Collect message from subsequent indented lines
            let mut message = String::new();
            let mut j = i + 1;
            while j < lines.len() {
                let next = lines[j];
                let trimmed = next.trim();
                if trimmed.is_empty() {
                    j += 1;
                    continue;
                }
                // Stop if we hit another error pattern or Call Stack
                if gcc_re.is_match(trimmed) || cmake_re.is_match(trimmed) || trimmed.starts_with("Call Stack") {
                    break;
                }
                if !message.is_empty() {
                    message.push('\n');
                }
                message.push_str(trimmed);
                j += 1;
            }

            errors.push(crate::commands::build::CompileError {
                file,
                line: line_num,
                column: 0,
                error_type,
                message,
            });
        }

        i += 1;
    }

    // Fallback: if no structured errors found, extract lines containing "error" keyword
    // This handles unrecognized formats like toolchain version mismatch, ninja failures, etc.
    if errors.is_empty() {
        for line in &lines {
            let lower = line.to_lowercase();
            if lower.contains("error") && !lower.contains("without error") && !lower.contains("no error")
                && !lower.contains("permission denied") && !lower.contains("filenotfounderror")
                && !lower.contains("no such file or directory")
                && !lower.contains("errno")
            {
                let msg = if line.len() > 500 {
                    format!("{}...", &line[..500])
                } else {
                    line.to_string()
                };
                errors.push(crate::commands::build::CompileError {
                    file: String::new(),
                    line: 0,
                    column: 0,
                    error_type: "error".to_string(),
                    message: msg,
                });
                if errors.len() >= 10 {
                    break;
                }
            }
        }
    }

    errors
}

#[tauri::command]
pub fn idf_monitor(project_path: String, idf_path: String, port: String, baudrate: u32) -> Result<String, String> {
    info!("Starting monitor: {} at {} baud", port, baudrate);
    monitor(&project_path, &idf_path, &port, baudrate)
}

#[tauri::command]
pub fn idf_menuconfig(project_path: String, idf_path: String) -> Result<String, String> {
    menuconfig(&project_path, &idf_path)
}

#[tauri::command]
pub async fn idf_clean(project_path: String, idf_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || clean(&project_path, &idf_path)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn idf_fullclean(project_path: String, idf_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || fullclean(&project_path, &idf_path)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn idf_size(project_path: String, idf_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || size(&project_path, &idf_path)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn idf_erase_flash(project_path: String, idf_path: String, port: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || erase_flash(&project_path, &idf_path, &port)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn idf_build_flash_monitor(project_path: String, idf_path: String, port: String) -> Result<String, String> {
    build_flash_monitor(&project_path, &idf_path, &port)
}

#[tauri::command]
pub fn idf_get_eim_setups() -> Result<Vec<serde_json::Value>, String> {
    let setups = detect_eim_setups();
    let result: Vec<serde_json::Value> = setups.iter().map(|s| {
        serde_json::json!({
            "name": s.name,
            "path": s.path,
            "toolsPath": s.idf_tools_path,
            "python": s.python,
        })
    }).collect();
    Ok(result)
}

/// 获取 IDF 支持的芯片目标列表（参考官方 getTargets.ts）
#[tauri::command]
pub fn idf_get_supported_targets(idf_path: Option<String>) -> Result<Vec<ChipTargetInfo>, String> {
    let path = idf_path.ok_or("IDF path is required")?;
    Ok(parse_supported_targets(&path))
}

/// 列出 IDF 内置示例项目模板（参考官方 newProject 面板）
#[tauri::command]
pub fn idf_list_templates(idf_path: String) -> Result<Vec<serde_json::Value>, String> {
    let examples_dir = Path::new(&idf_path).join("examples").join("get-started");
    if !examples_dir.exists() {
        return Ok(vec![
            serde_json::json!({"name": "blank", "label": "Blank Project", "description": "Empty ESP-IDF project with CMakeLists.txt and main.c"}),
        ]);
    }

    let mut templates = vec![
        serde_json::json!({"name": "blank", "label": "Blank Project", "description": "Empty ESP-IDF project with CMakeLists.txt and main.c"}),
    ];

    if let Ok(entries) = std::fs::read_dir(&examples_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let cmake = path.join("CMakeLists.txt");
                let main = path.join("main").join("CMakeLists.txt");
                if cmake.exists() || main.exists() {
                    let readme = path.join("README.md");
                    let desc = if readme.exists() {
                        std::fs::read_to_string(&readme)
                            .unwrap_or_default()
                            .lines()
                            .next()
                            .unwrap_or(&name)
                            .trim_start_matches("# ")
                            .to_string()
                    } else {
                        format!("ESP-IDF example: {}", name)
                    };
                    templates.push(serde_json::json!({
                        "name": format!("get-started/{}", name),
                        "label": name.split('-').map(|w| {
                            let mut c = w.chars();
                            match c.next() {
                                None => String::new(),
                                Some(f) => f.to_uppercase().chain(c).collect()
                            }
                        }).collect::<Vec<_>>().join(" "),
                        "description": desc,
                        "path": path.to_string_lossy(),
                    }));
                }
            }
        }
    }

    Ok(templates)
}

/// 读取和解析分区表 CSV（参考官方 partition-table 编辑器）
#[tauri::command]
pub fn idf_read_partition_table(project_path: String, _idf_path: Option<String>) -> Result<serde_json::Value, String> {
    let proj = Path::new(&project_path);
    // Find partition table CSV (prioritize project-specific, then sdkconfig default)
    let candidates = [
        proj.join("partitions.csv"),
        proj.join("partitions_singleapp.csv"),
        proj.join("build").join("partition_table").join("partitions.csv"),
    ];
    let csv_path = candidates.iter().find(|p| p.exists())
        .ok_or_else(|| "No partition table found. Create partitions.csv in your project.".to_string())?;

    let content = std::fs::read_to_string(csv_path).map_err(|e| format!("Failed to read: {}", e))?;
    let rows = parse_partition_csv(&content);

    Ok(serde_json::json!({
        "path": csv_path.to_string_lossy(),
        "raw": content,
        "partitions": rows,
    }))
}

fn parse_partition_csv(content: &str) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = trimmed.split(',').map(|s| s.trim().trim_matches('"')).collect();
        if cols.len() >= 5 {
            rows.push(serde_json::json!({
                "name": cols[0],
                "type": cols[1],
                "subtype": cols[2],
                "offset": cols[3],
                "size": cols[4],
                "flags": cols.get(5).copied().unwrap_or(""),
            }));
        }
    }
    rows
}

/// 组件管理：列出已安装的组件（参考官方 component-manager）
#[tauri::command]
pub fn idf_component_list(project_path: String, idf_path: String) -> Result<serde_json::Value, String> {
    let _ = idf_path; // reserved for future filtering
    
    // ESP-IDF 5.x 支持在项目根目录或组件目录放置 idf_component.yml
    let root_yml = Path::new(&project_path).join("idf_component.yml");
    let main_yml = Path::new(&project_path).join("main").join("idf_component.yml");
    
    let mut components = Vec::new();

    // Check managed_components directory
    let managed_dir = Path::new(&project_path).join("managed_components");
    if managed_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&managed_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let yml = path.join("idf_component.yml");
                    let version = if yml.exists() {
                        std::fs::read_to_string(&yml)
                            .ok()
                            .and_then(|c| {
                                c.lines()
                                    .find(|l| l.trim().starts_with("version:"))
                                    .map(|l| l.trim().trim_start_matches("version:").trim_matches('"').to_string())
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    components.push(serde_json::json!({
                        "name": name.to_string(),
                        "version": version,
                        "path": path.to_string_lossy(),
                    }));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "root_yml_exists": root_yml.exists(),
        "root_yml": if root_yml.exists() { std::fs::read_to_string(&root_yml).unwrap_or_default() } else { String::new() },
        "main_yml_exists": main_yml.exists(),
        "main_yml": if main_yml.exists() { std::fs::read_to_string(&main_yml).unwrap_or_default() } else { String::new() },
        "components": components,
    }))
}

/// 组件管理：添加组件依赖（调用 idf.py add-dependency）
#[tauri::command]
pub async fn idf_component_add(
    project_path: String,
    idf_path: String,
    component_name: String,
    version: Option<String>,
) -> Result<serde_json::Value, String> {
    let ver_str = version.unwrap_or_else(|| "*".into());
    let result = run_idf_command_live(&project_path, &idf_path, &[
        "add-dependency",
        &format!("{}@{}", component_name, ver_str),
    ]);
    match result {
        Ok(output) => Ok(serde_json::json!({"success": true, "output": output})),
        Err(e) => Err(e),
    }
}

/// SDKConfig: 获取当前项目的 sdkconfig 配置摘要
#[tauri::command]
pub fn idf_get_sdkconfig(project_path: String) -> Result<serde_json::Value, String> {
    let sdkconfig = Path::new(&project_path).join("sdkconfig");
    let defaults = Path::new(&project_path).join("sdkconfig.defaults");

    let content = if sdkconfig.exists() {
        std::fs::read_to_string(&sdkconfig).map_err(|e| format!("Failed to read sdkconfig: {}", e))?
    } else {
        String::new()
    };

    // Parse key-value pairs (CONFIG_XXX=YYY)
    let mut configs = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
            let value = trimmed[eq_pos + 1..].trim().trim_matches('"').to_string();
            configs.push(serde_json::json!({"key": key, "value": value}));
        }
    }

    Ok(serde_json::json!({
        "path": sdkconfig.to_string_lossy(),
        "exists": sdkconfig.exists(),
        "defaults_exists": defaults.exists(),
        "config_count": configs.len(),
        "configs": configs,
    }))
}

// ===== P2: Advance本文功能 =====

/// Arduino 支持：将 Arduino 添加为 ESP-IDF 组件（参考官方 addArduinoComponent）
#[tauri::command]
pub fn idf_add_arduino(project_path: String, idf_path: String) -> Result<serde_json::Value, String> {
    info!("Adding Arduino as ESP-IDF component for {}", project_path);
    let result = run_idf_command(&project_path, &idf_path, &["add-dependency", "espressif/arduino-esp32"]);
    match result {
        Ok(output) => Ok(serde_json::json!({"success": true, "output": output})),
        Err(e) => Err(format!("Failed to add Arduino component: {}", e)),
    }
}

/// eFuse: 读取芯片 eFuse 摘要（参考官方 efuse/index.ts）
#[tauri::command]
pub fn idf_efuse_summary(project_path: String, idf_path: String) -> Result<serde_json::Value, String> {
    info!("Reading eFuse summary for {}", project_path);
    let result = run_idf_command(&project_path, &idf_path, &["efuse-summary"]);
    match result {
        Ok(output) => Ok(serde_json::json!({"success": true, "output": output})),
        Err(e) => Err(format!("efuse-summary failed: {}", e)),
    }
}

/// eFuse: 烧录 eFuse（需谨慎！）
#[tauri::command]
pub fn idf_efuse_burn(project_path: String, idf_path: String, efuse_name: String, value: String) -> Result<serde_json::Value, String> {
    info!("Burning eFuse {}={} for {}", efuse_name, value, project_path);
    let confirm = format!("Are you sure you want to burn eFuse {}={}? This operation is IRREVERSIBLE!", efuse_name, value);
    let result = run_idf_command(&project_path, &idf_path, &[
        "efuse-burn",
        &efuse_name,
        &value,
    ]);
    match result {
        Ok(output) => Ok(serde_json::json!({
            "success": true,
            "output": output,
            "warning": confirm,
        })),
        Err(e) => Err(format!("efuse-burn failed: {}", e)),
    }
}

/// 单元测试：搜索项目中的测试文件（参考官方 unitTest adapter）
#[tauri::command]
pub fn idf_find_tests(project_path: String) -> Result<serde_json::Value, String> {
    let test_dir = Path::new(&project_path).join("test");
    let mut tests = Vec::new();
    if test_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&test_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "c" || e == "cpp") {
                    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    tests.push(serde_json::json!({
                        "name": name,
                        "path": path.to_string_lossy(),
                    }));
                }
            }
        }
    }
    // Also check main/test directory
    let main_test = Path::new(&project_path).join("main").join("test");
    if main_test.exists() {
        if let Ok(entries) = std::fs::read_dir(&main_test) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "c" || e == "cpp") {
                    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    tests.push(serde_json::json!({
                        "name": name,
                        "path": path.to_string_lossy(),
                    }));
                }
            }
        }
    }
    Ok(serde_json::json!({"tests": tests, "count": tests.len()}))
}

/// App Tracing: 启动应用追踪（参考官方 appTrace + SystemView）
#[tauri::command]
pub fn idf_app_trace_start(project_path: String, idf_path: String, port: Option<String>) -> Result<serde_json::Value, String> {
    info!("Starting app trace for {}", project_path);
    let port_str = port.unwrap_or_else(|| "auto".into());
    let result = if port_str == "auto" {
        run_idf_command(&project_path, &idf_path, &["app_trace"])
    } else {
        run_idf_command(&project_path, &idf_path, &["-p", &port_str, "app_trace"])
    };
    match result {
        Ok(output) => Ok(serde_json::json!({"success": true, "output": output})),
        Err(e) => Err(format!("App trace failed: {}. Ensure CONFIG_APPTRACE_ENABLE=y and OpenOCD is running.", e)),
    }
}

#[tauri::command]
pub async fn idf_doctor(project_path: Option<String>, idf_path: Option<String>) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || doctor_internal(project_path, idf_path)).await.map_err(|e| e.to_string())?
}

pub fn doctor_internal(project_path: Option<String>, idf_path: Option<String>) -> Result<serde_json::Value, String> {
    let mut checks: Vec<serde_json::Value> = Vec::new();
    let mut pass = 0;
    let mut fail = 0;

    // 1. IDF path check
    if let Some(ref idf) = idf_path {
        let idf_exists = Path::new(idf).exists();
        let idf_py_exists = find_idf_py(idf).is_some();
        if idf_exists && idf_py_exists {
            checks.push(serde_json::json!({"name": "IDF Path", "status": "ok", "detail": idf}));
            pass += 1;
        } else {
            let detail = if !idf_exists { "IDF directory not found" } else { "idf.py not found in IDF directory" };
            checks.push(serde_json::json!({"name": "IDF Path", "status": "error", "detail": format!("{}: {}", idf, detail)}));
            fail += 1;
        }

        // 2. IDF version
        if idf_exists {
            let ver = get_idf_version(idf);
            checks.push(serde_json::json!({"name": "IDF Version", "status": "ok", "detail": ver}));
            pass += 1;
        }

        // 3. Python check
        if let Some(eim) = find_eim_setup_public(idf) {
            let py_exists = Path::new(&eim.python).exists();
            if py_exists {
                checks.push(serde_json::json!({"name": "Python (EIM)", "status": "ok", "detail": &eim.python}));
                pass += 1;
            } else {
                checks.push(serde_json::json!({"name": "Python (EIM)", "status": "error", "detail": format!("Python not found at {}", eim.python)}));
                fail += 1;
            }
        } else {
            let mut python_cmd = Command::new("python");
            python_cmd.arg("--version");
            #[cfg(windows)] { python_cmd.creation_flags(0x08000000); }
            let python_result = python_cmd.output();
            match python_result {
                Ok(o) if o.status.success() => {
                    let ver = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    checks.push(serde_json::json!({"name": "Python (System)", "status": "ok", "detail": ver}));
                    pass += 1;
                }
                _ => {
                    checks.push(serde_json::json!({"name": "Python", "status": "error", "detail": "No Python found. Install Python 3.8+ or use EIM."}));
                    fail += 1;
                }
            }
        }

        // 4. esptool.py check
        match find_esptool_py(idf) {
            Some(path) => {
                checks.push(serde_json::json!({"name": "esptool.py", "status": "ok", "detail": path.to_string_lossy()}));
                pass += 1;
            }
            None => {
                checks.push(serde_json::json!({"name": "esptool.py", "status": "warn", "detail": "Not found. Run 'idf.py install' to install tools."}));
                fail += 1;
            }
        }

        // 5. Supported targets
        let targets = parse_supported_targets(idf);
        let target_names: Vec<String> = targets.iter().map(|t| t.label.clone()).collect();
        checks.push(serde_json::json!({"name": "Supported Targets", "status": "ok", "detail": target_names.join(", ")}));
        pass += 1;
    } else {
        checks.push(serde_json::json!({"name": "IDF Path", "status": "error", "detail": "No IDF path configured. Go to Settings → ESP-IDF."}));
        fail += 1;
    }

    // 6. Project checks
    if let Some(ref proj) = project_path {
        let proj_path = Path::new(proj);
        if proj_path.exists() {
            checks.push(serde_json::json!({"name": "Project Path", "status": "ok", "detail": proj}));
            pass += 1;

            if proj_path.join("sdkconfig").exists() {
                checks.push(serde_json::json!({"name": "SDKConfig", "status": "ok", "detail": "sdkconfig exists"}));
                pass += 1;
            } else {
                checks.push(serde_json::json!({"name": "SDKConfig", "status": "warn", "detail": "sdkconfig not found. Run Build or menuconfig."}));
                fail += 1;
            }

            if proj_path.join("CMakeLists.txt").exists() {
                checks.push(serde_json::json!({"name": "CMakeLists.txt", "status": "ok", "detail": "Project build file exists"}));
                pass += 1;
            } else {
                checks.push(serde_json::json!({"name": "CMakeLists.txt", "status": "error", "detail": "Not found. Not a valid ESP-IDF project."}));
                fail += 1;
            }

            if proj_path.join("build").exists() {
                checks.push(serde_json::json!({"name": "Build Directory", "status": "ok", "detail": "build/ exists (already built)"}));
                pass += 1;
            } else {
                checks.push(serde_json::json!({"name": "Build Directory", "status": "info", "detail": "Not built yet. Run Build first."}));
            }
        } else {
            checks.push(serde_json::json!({"name": "Project Path", "status": "error", "detail": format!("Project path does not exist: {}", proj)}));
            fail += 1;
        }
    }

    let health = if fail == 0 { "healthy" } else if fail <= checks.len() / 4 { "warn" } else { "error" };
    Ok(serde_json::json!({
        "health": health,
        "pass": pass,
        "fail": fail,
        "total": checks.len(),
        "checks": checks,
    }))
}

// ==================== CLI 实时流式执行（输出到 stdout，供 exec_shell 使用） ====================

/// 同步执行 idf.py 命令，同时将 stdout/stderr 实时打印到控制台
///
/// 返回 (success: bool, full_stdout: String, full_stderr: String)
/// 不受 idf_log 文件 I/O 影响，避免与前端争抢文件
pub fn run_idf_command_live(
    project_path: &str,
    idf_path: &str,
    args: &[&str],
) -> Result<String, String> {
    let idf_path_sanitized = sanitize_idf_path(idf_path);
    let idf_path_str = idf_path_sanitized.as_str();
    info!("Running idf.py {:?} in {} (idf_path='{}')", args, project_path, idf_path_str);

    let idf_py = find_idf_py(idf_path_str)
        .ok_or_else(|| {
            let root_py = Path::new(idf_path_str).join("idf.py");
            format!("idf.py not found at {}", root_py.display())
        })?;

    // 尝试 EIM 方式
    if let Some(eim_setup) = find_eim_setup(idf_path_str) {
        let python_path = eim_setup.python.replace('/', "\\");
        let tools_path = &eim_setup.idf_tools_path;

        let eim_path_entries = build_eim_path_entries(tools_path);
        let python_scripts = Path::new(&python_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let system_path = std::env::var("PATH").unwrap_or_default();
        let new_path = if eim_path_entries.is_empty() {
            format!("{};{}", python_scripts, system_path)
        } else {
            format!("{};{};{}", eim_path_entries.join(";"), python_scripts, system_path)
        };
        let idf_python_env_path = Path::new(&python_path)
            .parent().and_then(|p| p.parent())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let esp_idf_version = get_idf_version_for_env(idf_path_str);
        let idf_tools_dir = format!("{}\\tools", idf_path_str);

        let mut cmd = Command::new(&python_path);
        cmd.arg(&idf_py)
            .args(args)
            .current_dir(project_path)
            .env("IDF_PATH", idf_path_str)
            .env("IDF_TOOLS_PATH", tools_path)
            .env("IDF_PYTHON_ENV_PATH", &idf_python_env_path)
            .env("ESP_IDF_VERSION", &esp_idf_version)
            .env("IDF_COMPONENT_MANAGER", "1")
            .env("PATH", &new_path)
            .env("PYTHONPATH", format!("{};{}", &idf_tools_dir, std::env::var("PYTHONPATH").unwrap_or_default()))
            .env("OPENOCD_SCRIPTS", format!("{}\\openocd-esp32", tools_path))
            .env("ESP_ROM_ELF_DIR", format!("{}\\components\\esp_rom\\{}", idf_path_str,
                detect_target_from_project(project_path).unwrap_or_else(|| "esp32".to_string())))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)] { cmd.creation_flags(0x08000000); }

        return spawn_and_stream_live(cmd);
    }

    // 回退：export.bat 方式
    let args_str = args.iter()
        .map(|a| format!("\"{}\"", a))
        .collect::<Vec<_>>()
        .join(" ");

    let export_bat = Path::new(idf_path_str).join("export.bat");
    if export_bat.exists() {
        let cmd_str = format!(
            "call {} >nul && set ESP_IDF_VERSION={} && set IDF_COMPONENT_MANAGER=1 && python \"{}\" {}",
            export_bat.display(),
            get_idf_version_for_env(idf_path_str),
            idf_py.display(),
            args_str
        );
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &cmd_str])
            .current_dir(project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)] { cmd.creation_flags(0x08000000); }
        return spawn_and_stream_live(cmd);
    }

    let export_sh = Path::new(idf_path_str).join("export.sh");
    if export_sh.exists() {
        let cmd_str = format!(
            "source \"{}\" >/dev/null 2>&1 && export ESP_IDF_VERSION={} && export IDF_COMPONENT_MANAGER=1 && python \"{}\" {}",
            export_sh.display(),
            get_idf_version_for_env(idf_path_str),
            idf_py.display(),
            args_str
        );
        let mut cmd = Command::new("bash");
        cmd.args(["-c", &cmd_str])
            .current_dir(project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        return spawn_and_stream_live(cmd);
    }

    Err(format!("No ESP-IDF environment found at {}", idf_path_str))
}

/// 启动子进程，实时打印 stdout/stderr，最后返回完整输出摘要
fn spawn_and_stream_live(mut cmd: Command) -> Result<String, String> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)] { cmd.creation_flags(0x08000000); }
    let mut child = cmd.spawn()
        .map_err(|e| format!("Failed to spawn process: {}", e))?;

    let stdout_reader = child.stdout.take().map(BufReader::new);
    let stderr_reader = child.stderr.take().map(BufReader::new);

    let stdout_lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let stderr_lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    // Spawn stdout reader thread
    if let Some(reader) = stdout_reader {
        let lines = stdout_lines.clone();
        std::thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                    println!("{}", line);
                    lines.lock().unwrap().push(line);
            }
        });
    }

    // Spawn stderr reader thread
    if let Some(reader) = stderr_reader {
        let lines = stderr_lines.clone();
        std::thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                    eprintln!("{}", line);
                    lines.lock().unwrap().push(line);
            }
        });
    }

    let status = child.wait()
        .map_err(|e| format!("Failed to wait for process: {}", e))?;

    // Give reader threads a moment to finish
    std::thread::sleep(std::time::Duration::from_millis(300));

    let stdout_text = stdout_lines.lock().unwrap().join("\n");
    let stderr_text = stderr_lines.lock().unwrap().join("\n");
    let combined = if stderr_text.is_empty() {
        stdout_text.clone()
    } else if stdout_text.is_empty() {
        stderr_text.clone()
    } else {
        format!("{}\n{}", stdout_text, stderr_text)
    };

    if status.success() {
        Ok(combined)
    } else {
        Err(combined)
    }
}

#[tauri::command]
pub fn validate_python_path(path: String) -> Result<String, String> {
    let py_path = Path::new(&path);
    if !py_path.exists() {
        return Err("Python 路径不存在".to_string());
    }

    let mut cmd = std::process::Command::new(&path);
    cmd.args(["--version"]);
    #[cfg(windows)]
    { cmd.creation_flags(0x08000000); }
    let output = cmd.output()
        .map_err(|e| format!("无法执行 Python: {}", e))?;

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let result = if !version.is_empty() { version } else { stderr };

    if result.to_lowercase().contains("python") {
        Ok(result)
    } else {
        Err(format!("不是有效的 Python 可执行文件: {}", result))
    }
}