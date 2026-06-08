//! Git 集成命令模块

use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::{info, warn};

/// 文件状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStatus {
    pub path: String,
    pub status: String, // "modified", "added", "deleted", "untracked"
}

/// 获取 Git 状态
#[tauri::command]
pub async fn get_status(project_path: String) -> Result<Vec<FileStatus>, String> {
    info!("Getting git status for: {}", project_path);

    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&project_path)
        .output()
        .map_err(|e| {
            warn!("Git not available or not a git repository: {}", e);
            e.to_string()
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut files = Vec::new();

    for line in stdout.lines() {
        if line.len() >= 3 {
            let status_code = &line[..2];
            let path = line[3..].to_string();

            let status = match status_code.trim() {
                "M" | "MM" => "modified",
                "A" | "AM" => "added",
                "D" => "deleted",
                "??" => "untracked",
                _ => "unknown",
            };

            files.push(FileStatus {
                path,
                status: status.to_string(),
            });
        }
    }

    Ok(files)
}

/// 开始 AI 审核会话（创建分支）
#[tauri::command]
pub async fn start_ai_session(project_path: String) -> Result<String, String> {
    info!("Starting AI review session for: {}", project_path);

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let branch_name = format!("ai-review-{}", timestamp);

    // 创建新分支
    let output = Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .current_dir(&project_path)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(branch_name)
}

/// 提交 AI 修改
#[tauri::command]
pub async fn commit_ai_changes(
    project_path: String,
    message: String,
) -> Result<(), String> {
    info!("Committing AI changes: {}", message);

    // 添加所有变更
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(&project_path)
        .output();

    // 提交
    let output = Command::new("git")
        .args(["commit", "-m", &message])
        .current_dir(&project_path)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // 切回主分支并合并
    let _ = Command::new("git")
        .args(["checkout", "main"])
        .current_dir(&project_path)
        .output();

    let output = Command::new("git")
        .args(["merge", &format!("--no-ff -m \"{}\"", message)])
        .current_dir(&project_path)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}

/// 回退 AI 修改
#[tauri::command]
pub async fn revert_ai_changes(project_path: String) -> Result<(), String> {
    info!("Reverting AI changes for: {}", project_path);

    // 获取当前分支
    let output = Command::new("git")
        .args(["branch", "--current"])
        .current_dir(&project_path)
        .output()
        .map_err(|e| e.to_string())?;

    let current_branch = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();

    // 如果是 AI 分支，切回主分支并删除
    if current_branch.starts_with("ai-review-") {
        let _ = Command::new("git")
            .args(["checkout", "main"])
            .current_dir(&project_path)
            .output();

        let _ = Command::new("git")
            .args(["branch", "-D", &current_branch])
            .current_dir(&project_path)
            .output();
    }

    Ok(())
}
