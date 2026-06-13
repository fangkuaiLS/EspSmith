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
        cmd.arg("--thinking"); // 启用 reasoning/thinking 事件输出

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
        // MiMo-Code 内置免费通道（mimo/mimo-auto），无需 API Key
        // 只有使用第三方模型（如 xiaomi/*, deepseek/*）时才需要
        if config.model.starts_with("mimo/") {
            return None;
        }
        config.api_key.as_ref().map(|k| ("DEEPSEEK_API_KEY".into(), k.clone()))
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

// ─── MiMo-Code 事件格式转换 ────────────────────────────────────────────
//
// MiMo-Code `--format json` 输出格式与 CodeWhale `--output-format stream-json` 不同。
// 此函数将 MiMo-Code 事件转换为 CodeWhale 兼容格式，使 ai_assistant.rs 的解析逻辑无需修改。
//
// MiMo-Code 事件格式:
//   {"type":"text","timestamp":...,"sessionID":"...","part":{"type":"text","text":"..."}}
//   {"type":"tool_use","timestamp":...,"sessionID":"...","part":{"type":"tool","tool":"bash","callID":"...","state":{"status":"completed","input":{...},"output":"..."}}}
//   {"type":"step_start","timestamp":...,"sessionID":"...","part":{...}}
//   {"type":"step_finish","timestamp":...,"sessionID":"...","part":{"type":"step-finish","tokens":{...},"cost":...}}
//   {"type":"error","timestamp":...,"sessionID":"...","error":{...}}
//
// CodeWhale 兼容格式:
//   {"type":"content","content":"..."}
//   {"type":"tool_use","name":"...","id":"...","input":{...}}
//   {"type":"tool_result","id":"...","output":"..."}
//   {"type":"usage","input_tokens":...,"output_tokens":...}
//   {"type":"done","session_id":"..."}

pub fn convert_mimo_event(raw: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    let event_type = raw["type"].as_str().unwrap_or("");
    let mut results = Vec::new();

    match event_type {
        "text" => {
            // MiMo: {"type":"text","part":{"type":"text","text":"..."}}
            // CodeWhale: {"type":"content","content":"..."}
            if let Some(text) = raw["part"]["text"].as_str() {
                if !text.is_empty() {
                    results.push(serde_json::json!({
                        "type": "content",
                        "content": text
                    }));
                }
            }
        }
        "tool_use" => {
            // MiMo: {"type":"tool_use","part":{"type":"tool","tool":"bash","callID":"...","state":{"status":"completed","input":{...},"output":"..."}}}
            // CodeWhale: {"type":"tool_use","name":"...","id":"...","input":{...}} + {"type":"tool_result","id":"...","output":"..."}
            let part = &raw["part"];
            let tool_name = part["tool"].as_str().unwrap_or("unknown");
            let call_id = part["callID"].as_str().unwrap_or("");
            let state = &part["state"];
            let input = state.get("input").cloned().unwrap_or(serde_json::json!({}));

            results.push(serde_json::json!({
                "type": "tool_use",
                "name": tool_name,
                "id": call_id,
                "input": input
            }));

            // 如果工具已完成，同时发出 tool_result
            let status = state["status"].as_str().unwrap_or("");
            if status == "completed" {
                let output = state["output"].as_str().unwrap_or("");
                results.push(serde_json::json!({
                    "type": "tool_result",
                    "id": call_id,
                    "output": output
                }));
            } else if status == "error" {
                let error = state["error"].as_str().unwrap_or("unknown error");
                results.push(serde_json::json!({
                    "type": "tool_result",
                    "id": call_id,
                    "output": format!("Error: {}", error)
                }));
            }
        }
        "step_finish" => {
            // MiMo: {"type":"step_finish","part":{"type":"step-finish","tokens":{"input":...,"output":...,"cache":{"read":...,"write":...}},"cost":...}}
            // 映射为 usage + step_done（而非 done），因为 MiMo-Code 多轮调用会有多个 step_finish，
            // 只有子进程退出才是真正的完成。
            let part = &raw["part"];
            let tokens = &part["tokens"];
            let input_tokens = tokens["input"].as_u64().unwrap_or(0);
            let output_tokens = tokens["output"].as_u64().unwrap_or(0);
            let cache_read = tokens["cache"]["read"].as_u64().unwrap_or(0);

            results.push(serde_json::json!({
                "type": "usage",
                "input_tokens": input_tokens + cache_read,
                "output_tokens": output_tokens,
                "cached_tokens": cache_read
            }));

            // step_done 表示一轮完成但不是最终完成，主循环不会 break
            let session_id = raw["sessionID"].as_str().unwrap_or("");
            results.push(serde_json::json!({
                "type": "step_done",
                "session_id": session_id
            }));
        }
        "error" => {
            // MiMo: {"type":"error","error":{...}}
            // 转为 CodeWhale 兼容的 done 事件 + 将错误信息放入 output
            let error_msg = if let Some(msg) = raw["error"]["data"]["message"].as_str() {
                msg.to_string()
            } else if let Some(name) = raw["error"]["name"].as_str() {
                name.to_string()
            } else {
                "Unknown MiMo-Code error".to_string()
            };
            tracing::error!("MiMo-Code error: {}", error_msg);
            // 产生一个 content 事件让 output 非空，这样错误信息能传递到前端
            results.push(serde_json::json!({
                "type": "content",
                "content": format!("❌ MiMo-Code 错误: {}", error_msg)
            }));
            // 同时产生 done 事件结束循环
            let session_id = raw["sessionID"].as_str().unwrap_or("");
            results.push(serde_json::json!({
                "type": "done",
                "session_id": session_id
            }));
        }
        "step_start" => {
            // step_start 不需要转换为 CodeWhale 格式
        }
        "reasoning" => {
            // 将 MiMo-Code 的 reasoning 事件转换为统一格式，供前端显示
            if let Some(text) = raw.get("part")
                .and_then(|p| p.get("text"))
                .or_else(|| raw.get("content"))
                .or_else(|| raw.get("text"))
                .and_then(|v| v.as_str())
            {
                results.push(serde_json::json!({
                    "type": "reasoning",
                    "content": text,
                }));
            }
        }
        _ => {
            // 未知事件类型，原样传递（让 ai_assistant 的默认分支处理）
            results.push(raw.clone());
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}
