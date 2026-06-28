//! Kconfig menu tree loader — runs kconfig_dump.py to get the full menu structure.
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::idf;
use tracing::info;

const KCONFIG_DUMP_PY: &str = include_str!("../scripts/kconfig_dump.py");

pub fn menu_from_kconfig(project_path: &str, idf_path: &str) -> Result<serde_json::Value, String> {
    let idf_py = idf::find_idf_py(idf_path)
        .ok_or_else(|| format!("idf.py not found in {}", idf_path))?;

    let script_path = write_script_temp()?;
    let sdkconfig_path = Path::new(project_path).join("sdkconfig");
    let sdkconfig_arg = if sdkconfig_path.exists() {
        sdkconfig_path.to_string_lossy().to_string()
    } else {
        String::new()
    };

    info!("[menu_from_kconfig] Running kconfig_dump.py for {}", project_path);
    let mut child = start_python_script(project_path, idf_path, &idf_py, &script_path, &sdkconfig_arg)?;
    let stdout = child.stdout.take().ok_or("Script stdout not available")?;
    let mut reader = BufReader::new(stdout);
    let mut json_str = String::new();
    reader.read_to_string(&mut json_str)
        .map_err(|e| format!("Failed to read kconfig_dump.py output: {}", e))?;

    let stderr = drain_stderr(&mut child);
    let _ = child.kill();
    let _ = child.wait();
    if !stderr.is_empty() { info!("[menu_from_kconfig] stderr:\n{}", stderr); }
    if json_str.trim().is_empty() {
        return Err(format!("kconfig_dump.py produced no output. stderr: {}", stderr));
    }
    serde_json::from_str(json_str.trim())
        .map_err(|e| format!("JSON parse error: {}", e))
}

fn write_script_temp() -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("espsmith");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create temp dir: {}", e))?;
    let path = dir.join("kconfig_dump.py");

    let cwd = std::env::current_dir().unwrap_or_default();
    let s1 = cwd.join("src-tauri").join("scripts").join("kconfig_dump.py");
    let s2 = cwd.join("scripts").join("kconfig_dump.py");
    let dev_script = if s1.exists() { s1 } else { s2 };

    let content = if dev_script.exists() {
        std::fs::read_to_string(&dev_script)
            .map_err(|e| format!("Cannot read source: {}", e))?
    } else {
        KCONFIG_DUMP_PY.to_string()
    };

    std::fs::write(&path, &content)
        .map_err(|e| format!("Cannot write script: {}", e))?;
    Ok(path)
}

fn start_python_script(
    project_path: &str, idf_path: &str, _idf_py: &Path,
    script_path: &Path, sdkconfig_arg: &str,
) -> Result<Child, String> {
    if let Some(eim_setup) = idf::find_eim_setup(idf_path) {
        let python = idf::normalize_path_sep(&eim_setup.python);
        if !Path::new(&python).exists() {
            return Err(format!("EIM Python not found: {}", python));
        }
        let idf_tools = idf::join_path_parts(&[idf_path, "tools"]);
        let sys_path = std::env::var("PATH").unwrap_or_default();
        let py_scripts = Path::new(&python).parent()
            .map(|p| p.to_string_lossy().to_string()).unwrap_or_default();

        let mut cmd = Command::new(&python);
        cmd.arg(script_path).arg(idf_path).arg(project_path).arg(sdkconfig_arg)
            .env("IDF_PATH", idf_path)
            .env("IDF_TOOLS_PATH", &eim_setup.idf_tools_path)
            .env("ESP_IDF_VERSION", idf::get_idf_version_for_env(idf_path))
            .env("PATH", format!("{}{}{}", py_scripts, idf::PATH_LIST_SEP, sys_path))
            .env("PYTHONPATH", format!("{}{}{}", &idf_tools, idf::PATH_LIST_SEP, std::env::var("PYTHONPATH").unwrap_or_default()))
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)] { cmd.creation_flags(0x08000000); }
        cmd.spawn().map_err(|e| format!("Failed to run kconfig_dump.py: {}", e))
    } else {
        if cfg!(windows) {
            let export_bat = Path::new(idf_path).join("export.bat");
            if !export_bat.exists() {
                return Err(format!("export.bat not found at {}", export_bat.display()));
            }
            let cmd_str = format!(
                "call \"{}\" >nul 2>&1 && set ESP_IDF_VERSION={} && python \"{}\" \"{}\" \"{}\" \"{}\"",
                export_bat.display(), idf::get_idf_version_for_env(idf_path),
                script_path.display(), idf_path, project_path, sdkconfig_arg,
            );
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", &cmd_str]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
            #[cfg(windows)] { cmd.creation_flags(0x08000000); }
            cmd.spawn().map_err(|e| format!("Failed to run kconfig_dump.py: {}", e))
        } else {
            Err("Non-Windows not supported".to_string())
        }
    }
}

fn drain_stderr(child: &mut Child) -> String {
    if let Some(ref mut stderr) = child.stderr {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    } else { String::new() }
}
