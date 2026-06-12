//! AI Provider 抽象层
//!
//! 将 CodeWhale / MiMo-Code 等 AI 编码助手的子进程集成抽象为统一的 trait。
//! 核心设计：所有 Provider 都通过 `run` 子命令以非交互模式执行，stdout 输出 JSON 事件流。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::process::Command;

use crate::ai_assistant::AIConfig;

/// Provider 类型标识
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    CodeWhale,
    MiMoCode,
}

impl ProviderKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            "mimo" | "mimocode" | "mimo-code" => ProviderKind::MiMoCode,
            _ => ProviderKind::CodeWhale,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::CodeWhale => "codewhale",
            ProviderKind::MiMoCode => "mimo",
        }
    }
}

/// Provider 构建命令的结果
#[allow(dead_code)]
pub struct ProviderCommand {
    /// 已配置好 args/env/stdio 的 tokio Command
    pub cmd: Command,
    /// Provider 显示名称（用于日志）
    pub display_name: &'static str,
}

/// AI Provider trait：定义子进程模式 AI 编码助手的统一接口
#[allow(async_fn_in_trait)]
#[allow(dead_code)]
pub trait AIProvider: Send + Sync {
    /// Provider 类型
    fn kind(&self) -> ProviderKind;

    /// 显示名称
    fn display_name(&self) -> &'static str;

    /// 检查 Provider 二进制是否可用，返回二进制路径
    fn ensure_ready(&self) -> Result<PathBuf, String>;

    /// 检查安装状态（供前端查询）
    fn check_status(&self) -> String;

    /// 构建 spawn 命令
    ///
    /// 包含：二进制路径、子命令、参数、环境变量、工作目录、stdio 配置
    fn build_command(
        &self,
        binary: &PathBuf,
        config: &AIConfig,
        prompt: &str,
        session_id: Option<&str>,
    ) -> ProviderCommand;

    /// 获取 API Key 环境变量名和值
    fn api_key_env(&self, config: &AIConfig) -> Option<(String, String)>;

    /// 是否支持 session 持续（--session-id / --continue 等）
    fn supports_session(&self) -> bool;

    /// 获取 session 参数的 flag 名（如 "--session-id" 或 "--session"）
    fn session_flag(&self) -> &'static str;
}

// ─── CodeWhale Provider ────────────────────────────────────────────

pub struct CodeWhaleProvider;

impl AIProvider for CodeWhaleProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::CodeWhale
    }

    fn display_name(&self) -> &'static str {
        "CodeWhale"
    }

    fn ensure_ready(&self) -> Result<PathBuf, String> {
        crate::ai_assistant::ensure_codewhale_ready()
    }

    fn check_status(&self) -> String {
        // 同步版本，直接检查路径
        if let Some(bundled) = crate::ai_assistant::get_bundled_codewhale_binary() {
            if bundled.exists() {
                return "local".into();
            }
        }
        let local = crate::ai_assistant::get_local_codewhale_binary();
        if local.exists() {
            return "local".into();
        }
        if crate::ai_assistant::which_cmd("codewhale").is_some() {
            return "system".into();
        }
        "missing".into()
    }

    fn build_command(
        &self,
        binary: &PathBuf,
        config: &AIConfig,
        prompt: &str,
        session_id: Option<&str>,
    ) -> ProviderCommand {
        let mut cmd = Command::new(binary);
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd.args(["--model", &config.model, "exec", "--auto", "--output-format", "stream-json"]);

        if let Some(sid) = session_id {
            cmd.arg("--session-id").arg(sid);
        }

        // prompt 作为位置参数
        cmd.arg(prompt);

        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        ProviderCommand {
            cmd,
            display_name: "CodeWhale",
        }
    }

    fn api_key_env(&self, config: &AIConfig) -> Option<(String, String)> {
        match config.ai_provider.as_str() {
            "ollama" => None,
            _ => config.api_key.as_ref().map(|k| ("DEEPSEEK_API_KEY".into(), k.clone())),
        }
    }

    fn supports_session(&self) -> bool {
        true
    }

    fn session_flag(&self) -> &'static str {
        "--session-id"
    }
}

// ─── MiMo-Code Provider ────────────────────────────────────────────

pub struct MiMoCodeProvider;

impl AIProvider for MiMoCodeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::MiMoCode
    }

    fn display_name(&self) -> &'static str {
        "MiMo-Code"
    }

    fn ensure_ready(&self) -> Result<PathBuf, String> {
        // MiMo-Code 通过 npm 全局安装，二进制名为 mimo
        for name in &["mimo", "mimo.cmd"] {
            if let Some(path) = crate::ai_assistant::which_cmd(name) {
                tracing::info!("Using system MiMo-Code: {}", path.display());
                return Ok(path);
            }
        }
        Err("i18n:aiBackend.mimoNotFound".into())
    }

    fn check_status(&self) -> String {
        if crate::ai_assistant::which_cmd("mimo").is_some() {
            return "system".into();
        }
        if crate::ai_assistant::which_cmd("mimo.cmd").is_some() {
            return "system".into();
        }
        "missing".into()
    }

    fn build_command(
        &self,
        binary: &PathBuf,
        config: &AIConfig,
        prompt: &str,
        session_id: Option<&str>,
    ) -> ProviderCommand {
        let mut cmd = Command::new(binary);
        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // mimo run [message..] --format json --dangerously-skip-permissions --model provider/model
        cmd.arg("run");

        // 如果有 session，用 --continue 继续
        if let Some(_sid) = session_id {
            cmd.arg("--continue");
        }

        cmd.arg("--format").arg("json");
        cmd.arg("--dangerously-skip-permissions");

        // MiMo-Code 的 model 格式为 provider/model
        let model_arg = if config.model.contains('/') {
            config.model.clone()
        } else {
            // 根据 ai_provider 推导前缀
            match config.ai_provider.as_str() {
                "ollama" => format!("ollama/{}", config.model),
                _ => format!("deepseek/{}", config.model),
            }
        };
        cmd.arg("--model").arg(&model_arg);

        // prompt 作为 positional argument
        cmd.arg(prompt);

        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        ProviderCommand {
            cmd,
            display_name: "MiMo-Code",
        }
    }

    fn api_key_env(&self, config: &AIConfig) -> Option<(String, String)> {
        match config.ai_provider.as_str() {
            "ollama" => None,
            _ => config.api_key.as_ref().map(|k| ("DEEPSEEK_API_KEY".into(), k.clone())),
        }
    }

    fn supports_session(&self) -> bool {
        true
    }

    fn session_flag(&self) -> &'static str {
        "--continue"
    }
}

/// 根据 AIConfig 中的 ai_provider 字段选择对应的 Provider
pub fn select_provider(config: &AIConfig) -> Box<dyn AIProvider> {
    match ProviderKind::from_str(&config.ai_provider) {
        ProviderKind::MiMoCode => Box::new(MiMoCodeProvider),
        ProviderKind::CodeWhale => Box::new(CodeWhaleProvider),
    }
}
