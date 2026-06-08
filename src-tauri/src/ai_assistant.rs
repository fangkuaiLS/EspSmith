use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::info;
use crate::connection::ConnectionMode;

/// 内嵌的 CodeWhale 二进制目录路径（由 lib.rs 在 setup 时初始化）
static BUNDLED_CODEWHALE_DIR: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AIStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "thinking")]
    Thinking,
    #[serde(rename = "tool_call")]
    ToolCall,
    #[serde(rename = "building")]
    Building,
    #[serde(rename = "flashing")]
    Flashing,
    #[serde(rename = "error")]
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub total_tokens: u64,
    pub cost_rmb: f64,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AICumulativeUsage {
    pub session: AIUsage,
    pub last_message: AIUsage,
    pub message_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Full,
    Ask,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Full
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIConfig {
    pub model: String,
    pub api_key: Option<String>,
    pub ollama_endpoint: Option<String>,
    pub enable_tool_use: bool,
    pub project_path: Option<String>,
    pub idf_path: Option<String>,
    #[serde(default)]
    pub ael_path: Option<String>,
    #[serde(default)]
    pub target_chip: Option<String>,
    #[serde(default)]
    pub flash_port: Option<String>,
    #[serde(default = "default_ai_provider")]
    pub ai_provider: String,
    #[serde(default)]
    pub permission_mode: PermissionMode,
    /// Whether the chip has changed and the next build should include --target (set-target)
    #[serde(default)]
    pub chip_changed: bool,
}

fn default_ai_provider() -> String {
    "deepseek".into()
}

struct CodeWhaleClient {
    config: AIConfig,
    session_id: Option<String>,
    cumulative_input_tokens: u64,
    cumulative_output_tokens: u64,
    cumulative_cached_tokens: u64,
    cumulative_cost_rmb: f64,
    message_count: u64,
    pending_permission_request: Option<PendingPermissionRequest>,
    permission_response_tx: Option<tokio::sync::oneshot::Sender<bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingPermissionRequest {
    tool_name: String,
    tool_input: Option<serde_json::Value>,
    reason: String,
}

impl CodeWhaleClient {
    fn new() -> Self {
        Self {
            config: AIConfig {
                model: "deepseek-v4-flash".into(),
                api_key: None,
                ollama_endpoint: None,
                enable_tool_use: true,
                project_path: None,
                idf_path: None,
                ael_path: None,
                target_chip: None,
                flash_port: None,
                ai_provider: "deepseek".into(),
                permission_mode: PermissionMode::default(),
                chip_changed: false,
            },
            session_id: None,
            cumulative_input_tokens: 0,
            cumulative_output_tokens: 0,
            cumulative_cached_tokens: 0,
            cumulative_cost_rmb: 0.0,
            message_count: 0,
            pending_permission_request: None,
            permission_response_tx: None,
        }
    }

    fn add_usage(&mut self, input_tokens: u64, output_tokens: u64, cached_tokens: u64, model: &str) {
        let cost = calculate_cost_rmb(input_tokens, output_tokens, cached_tokens, model);
        self.cumulative_input_tokens += input_tokens;
        self.cumulative_output_tokens += output_tokens;
        self.cumulative_cached_tokens += cached_tokens;
        self.cumulative_cost_rmb += cost;
        self.message_count += 1;
    }

    fn reset_usage(&mut self) {
        self.cumulative_input_tokens = 0;
        self.cumulative_output_tokens = 0;
        self.cumulative_cached_tokens = 0;
        self.cumulative_cost_rmb = 0.0;
        self.message_count = 0;
    }
}

fn calculate_cost_rmb(input_tokens: u64, output_tokens: u64, cached_tokens: u64, model: &str) -> f64 {
    let (input_price, output_price, cache_price) = match model {
        m if m.contains("deepseek-v4-pro") => (3.0, 6.0, 0.5),
        m if m.contains("deepseek-v4-flash") => (1.0, 2.0, 0.5),
        _ => (1.0, 2.0, 0.5),
    };
    let uncached_input = input_tokens.saturating_sub(cached_tokens);
    let input_cost = (uncached_input as f64) / 1_000_000.0 * input_price;
    let cache_cost = (cached_tokens as f64) / 1_000_000.0 * cache_price;
    let output_cost = (output_tokens as f64) / 1_000_000.0 * output_price;
    input_cost + cache_cost + output_cost
}

lazy_static::lazy_static! {
    static ref AI_CLIENT: Arc<Mutex<CodeWhaleClient>> =
        Arc::new(Mutex::new(CodeWhaleClient::new()));
    static ref RUNNING_CHILD: Arc<Mutex<Option<tokio::process::Child>>> =
        Arc::new(Mutex::new(None));
    static ref AI_STATUS: Arc<Mutex<AIStatus>> =
        Arc::new(Mutex::new(AIStatus::Idle));
    static ref ACTIVE_JTAG_OPERATION: Arc<Mutex<Option<OperationProgress>>> =
        Arc::new(Mutex::new(None));
}

async fn kill_running_child() {
    let mut child_guard = RUNNING_CHILD.lock().await;
    if let Some(mut child) = child_guard.take() {
        info!("Stopping CodeWhale process");
        // On Windows, child.start_kill() only kills the direct child (CodeWhale),
        // leaving grandchild processes (espsmith.exe → idf.py) as orphans.
        // Use taskkill /F /T /PID to kill the entire process tree so that
        // running idf.py set-target / build are also terminated.
        #[cfg(target_os = "windows")]
        {
            if let Some(pid) = child.id() {
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = child.start_kill();
        }
        let _ = child.wait().await;
    }
}

#[tauri::command]
pub async fn ai_start(config: AIConfig) -> Result<String, String> {
    let mut client = AI_CLIENT.lock().await;
    let old_chip = client.config.target_chip.clone();
    let old_port = client.config.flash_port.clone();
    client.config = config;
    if client.config.target_chip.is_none() {
        client.config.target_chip = old_chip;
    }
    if client.config.flash_port.is_none() {
        client.config.flash_port = old_port;
    }
    client.session_id = None;

    let _ = client
        .config
        .api_key
        .clone()
        .ok_or_else(|| "Please configure an API Key in Settings first".to_string())?;

    if client.config.enable_tool_use {
        info!("MCP server check skipped — using exec_shell-only mode");
    }

    ensure_codewhale_ready().map_err(|e| {
        format!("i18n:aiBackend.codewhaleNotFound|error={}", e)
    })?;

    Ok("CodeWhale Agent is ready".into())
}

#[tauri::command]
pub async fn ai_stop() -> Result<String, String> {
    kill_running_child().await;
    Ok("stopped".into())
}

async fn emit_file_sync_events(
    app_handle: &tauri::AppHandle,
    project_path: Option<&str>,
    write_path: &str,
) {
    let _ = app_handle.emit("ai-file-changed", write_path);

    if let Some(pp) = project_path {
        let path = std::path::Path::new(write_path);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let is_hw_config = file_name == "hardware_config.json"
            && path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some(".espsmith");

        if is_hw_config {
            let config_path = std::path::Path::new(pp)
                .join(".espsmith")
                .join("hardware_config.json");
            if let Ok(config_str) = std::fs::read_to_string(&config_path) {
                if let Ok(config) =
                    serde_json::from_str::<crate::commands::hardware::HardwareConfig>(&config_str)
                {
                    // 重新生成 hardware_pins.h（exec_shell 模式下 AI write_file 不会走 MCP/ filesystem 路径）
                    let _ = crate::commands::hardware::generate_hardware_header(pp.to_string());
                    let _ = app_handle.emit("hw-config-changed", &config);
                    info!("Emitted hw-config-changed + regenerated hardware_pins.h after write_file: {}", write_path);
                }
            }
        }
    }
}

#[tauri::command]
pub async fn ai_send_message(
    message: String,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let (api_key, model, project_path, idf_path, enable_tool_use, ael_path, target_chip, flash_port, session_id, ai_provider, permission_mode, chip_changed) = {
        let client = AI_CLIENT.lock().await;
        let key = client
            .config
            .api_key
            .clone()
            .ok_or_else(|| "Please configure an API Key in Settings first".to_string())?;
        (
            key,
            client.config.model.clone(),
            client.config.project_path.clone(),
            client.config.idf_path.clone(),
            client.config.enable_tool_use,
            client.config.ael_path.clone(),
            client.config.target_chip.clone(),
            client.config.flash_port.clone(),
            client.session_id.clone(),
            client.config.ai_provider.clone(),
            client.config.permission_mode.clone(),
            client.config.chip_changed,
        )
    };

    kill_running_child().await;
    {
        let mut status = AI_STATUS.lock().await;
        *status = AIStatus::Thinking;
    }


    if enable_tool_use {
        info!("MCP server check skipped — using exec_shell-only mode");
    }

    if enable_tool_use {
        ensure_project_agent_instructions(project_path.as_deref(), idf_path.as_deref(), ael_path.as_deref())?;
    }

    let binary = ensure_codewhale_ready()?;
    let mut cmd = tokio::process::Command::new(&binary);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd.args(["--model", &model, "exec", "--auto", "--output-format", "stream-json"]);

    if let Some(ref sid) = session_id {
        cmd.arg("--session-id").arg(sid);
    }

    if let Some(ref pp) = project_path {
        let exp_dir = std::path::Path::new(pp).join(".espsmith").join("experience");
        crate::experience::init(exp_dir);
    }

    cmd.arg(build_short_agent_prompt(&message, project_path.as_deref(), idf_path.as_deref(), target_chip.as_deref(), flash_port.as_deref(), chip_changed));

    // Clear chip_changed flag after it's been consumed by the prompt
    if chip_changed {
        let mut client = AI_CLIENT.lock().await;
        client.config.chip_changed = false;
    }

    match ai_provider.as_str() {
        "ollama" => {
            info!("Using Ollama local model: {}", model);
        }
        _ => {
            cmd.env("DEEPSEEK_API_KEY", &api_key);
            info!("Using DeepSeek API with model: {}", model);
        }
    }

    if let Some(ref path) = project_path {
        cmd.current_dir(path);
        info!("CodeWhale working directory: {}", path);
    }

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    info!("Starting CodeWhale exec (session: {:?})", session_id);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("i18n:aiBackend.codewhaleStartFailed|path={}|error={}", binary.display(), e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to read CodeWhale stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to read CodeWhale stderr")?;

    {
        let mut guard = RUNNING_CHILD.lock().await;
        *guard = Some(child);
    }

    // 注册全局管道事件监听器（仅首次注册，后续调用复用）
    // 将 RunnerEvent 桥接到 OperationProgress 卡片，替代之前的假定时器。
    {
        static LISTENER_REGISTERED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        let _ = LISTENER_REGISTERED.get_or_init(|| {
            let ah = app_handle.clone();
            let listener: Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync> =
                Arc::new(move |event: &crate::self_healing::types::RunnerEvent| {
                    use crate::self_healing::types::RunnerEvent;
                    // 调试日志：记录每个收到的 RunnerEvent
                    tracing::info!(
                        "[GlobalListener] Received event: {:?}",
                        event
                    );
                    // 使用 try_lock 避免 IPC 线程和 tokio 异步运行时之间的死锁
                    let mut op = match ACTIVE_JTAG_OPERATION.try_lock() {
                        Ok(guard) => guard,
                        Err(_) => {
                            tracing::warn!("[GlobalListener] ACTIVE_JTAG_OPERATION lock contention, retrying...");
                            // 短暂等待后重试一次
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            match ACTIVE_JTAG_OPERATION.try_lock() {
                                Ok(guard) => guard,
                                Err(_) => {
                                    tracing::warn!("[GlobalListener] ACTIVE_JTAG_OPERATION still locked, dropping event");
                                    return;
                                }
                            }
                        }
                    };
                    let Some(ref mut active) = *op else {
                        tracing::warn!("[GlobalListener] Dropping event — no active operation");
                        return;
                    };

                    let step_idx = match event {
                        RunnerEvent::StepStarted { step_index, .. } |
                        RunnerEvent::StepFailed { step_index, .. } |
                        RunnerEvent::StepPassed { step_index, .. } |
                        RunnerEvent::RecoveryApplied { step_index, .. } => *step_index,
                    };

                    if step_idx >= active.steps.len() { return; }

                    match event {
                        RunnerEvent::StepStarted { .. } => {
                            for (i, s) in active.steps.iter_mut().enumerate() {
                                if i < step_idx { s.status = "done".into(); }
                                else if i == step_idx { s.status = "running".into(); }
                                else { s.status = "pending".into(); }
                            }
                        }
                        RunnerEvent::StepPassed { .. } => {
                            active.steps[step_idx].status = "done".into();
                        }
                        RunnerEvent::StepFailed { will_retry, error, .. } => {
                            if !*will_retry {
                                active.steps[step_idx].status = "error".into();
                            }
                            let _ = ah.emit("ai-operation-step-error", serde_json::json!({
                                "stepIndex": step_idx,
                                "error": error,
                                "willRetry": will_retry,
                            }));
                        }
                        RunnerEvent::RecoveryApplied { action, reason, .. } => {
                            let _ = ah.emit("ai-operation-recovery", serde_json::json!({
                                "action": action,
                                "reason": reason,
                            }));
                        }
                    }
                    // 调试日志：记录发出的进度事件
                    tracing::info!(
                        "[GlobalListener] Emitting ai-operation-progress: step_idx={}, steps={:?}",
                        step_idx,
                        active.steps.iter().map(|s| (&s.label, &s.status)).collect::<Vec<_>>()
                    );
                    let _ = ah.emit("ai-operation-progress", &*active);
                });
            crate::self_healing::add_global_listener(listener);
        });
    }

    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();
    let mut output = String::new();
    let mut stderr_lines = Vec::new();
    let mut new_session_id: Option<String> = None;
    let mut msg_input_tokens: u64 = 0;
    let mut msg_output_tokens: u64 = 0;
    let mut msg_cached_tokens: u64 = 0;
    let current_model = model.clone();
    let mut pending_write_path: Option<String> = None;

    loop {
        tokio::select! {
            result = stdout_reader.next_line() => {
                match result {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        // Strip terminal escape sequences that CodeWhale
                        // may interleave with JSON (e.g. OSC title escape \x1b]0;...\x07)
                        let cleaned = strip_ansi_escapes(&line);
                        match serde_json::from_str::<serde_json::Value>(&cleaned) {
                            Ok(event) => {
                                let event_type = event["type"].as_str().unwrap_or("");
                                match event_type {
                                    "content" => {
                                        if let Some(content) = event["content"].as_str() {
                                            info!("CodeWhale content ({} chars): {}",
                                                content.len(),
                                                &content[..content.len().min(120)]);
                                            output.push_str(content);
                                            let _ = app_handle.emit("ai-chunk", content);
                                        }
                                    }
                                    "tool_use" => {
                                        let name = event["name"].as_str().unwrap_or("unknown");
                                        let is_sensitive = is_sensitive_tool(name, event.get("input"));
                                        
                                        if is_sensitive && permission_mode == PermissionMode::Ask {
                                            let description = describe_sensitive_operation(name, event.get("input"));
                                            let pending = PendingPermissionRequest {
                                                tool_name: name.to_string(),
                                                tool_input: event.get("input").cloned(),
                                                reason: description.clone(),
                                            };
                                            let _ = app_handle.emit("ai-permission-request", serde_json::to_value(&pending).unwrap_or_default());
                                            
                                            let (tx, rx) = tokio::sync::oneshot::channel();
                                            {
                                                let mut client = AI_CLIENT.lock().await;
                                                client.pending_permission_request = Some(pending);
                                                client.permission_response_tx = Some(tx);
                                            }
                                            
                                            let _ = app_handle.emit("ai-chunk", format!("i18n:aiBackend.waitingConfirmation|description={}", description));
                                            
                                            match rx.await {
                                                Ok(true) => {
                                                    let _ = app_handle.emit("ai-chunk", "i18n:aiBackend.permissionAllowed");
                                                }
                                                Ok(false) | Err(_) => {
                                                    let _ = app_handle.emit("ai-chunk", "i18n:aiBackend.permissionDenied");
                                                    kill_running_child().await;
                                                    return Err("i18n:aiBackend.userRefused".into());
                                                }
                                            }
                                        }
                                        info!("CodeWhale tool call: {}", name);
                                        // 跟踪 write_file 调用的路径
                                        if name == "write_file" {
                                            pending_write_path = event.get("input")
                                                .and_then(|v| v.get("path"))
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                        } else {
                                            pending_write_path = None;
                                        }
                                        // 根据工具名称更新状态
                                        let new_status = if name.contains("build") {
                                            AIStatus::Building
                                        } else if name.contains("flash") {
                                            AIStatus::Flashing
                                        } else {
                                            AIStatus::ToolCall
                                        };
                                        {
                                            let mut status = AI_STATUS.lock().await;
                                            *status = new_status;
                                        }
                                        let _ = app_handle.emit("ai-tool-use", serde_json::json!({
                                            "name": name,
                                            "id": event["id"],
                                            "input": event["input"],
                                        }));
                                        if name == "exec_shell" {
                                            let cmd = event.get("input")
                                                .and_then(|v| v.get("command"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let tool_use_id = event["id"].as_str().unwrap_or("");
                                            // 判断当前是否为 JTAG 模式
                                            // 优先级：1. 缓存的连接模式 > 2. flash_port 是否为空（兜底）
                                            let is_jtag = {
                                                let conn = crate::connection::get_cached_connection_info();
                                                if conn.mode != ConnectionMode::Unknown {
                                                    conn.mode.is_jtag()
                                                } else {
                                                    // 兜底：无 UART 串口端口即为 JTAG
                                                    let c = AI_CLIENT.lock().await;
                                                    c.config.flash_port.as_ref().is_none_or(|p| p.trim().is_empty())
                                                }
                                            };
                                            tracing::info!(
                                                "[AIAssistant] Connection mode detection: is_jtag={}",
                                                is_jtag
                                            );
                                            if let Some(progress) = detect_jtag_operation(cmd, tool_use_id, is_jtag) {
                                                tracing::info!(
                                                    "[AIAssistant] Detected operation: type={}, steps={}, tool_use_id={}",
                                                    progress.operation_type,
                                                    progress.steps.len(),
                                                    tool_use_id
                                                );
                                                {
                                                    let mut op = ACTIVE_JTAG_OPERATION.lock().await;
                                                    *op = Some(progress.clone());
                                                }
                                                // 发送初始进度卡片（步骤全部 pending）
                                                let _ = app_handle.emit("ai-operation-progress", &progress);

                                                // 委托模式下，CLI 会将执行委托给主进程，
                                                // 主进程直接运行 Self-Healing 引擎，RunnerEvent 通过全局监听器实时更新进度。
                                                // 不再需要 start_progress_simulator。
                                            }
                                        }
                                    }
                                    "tool_result" => {
                                        // CodeWhale uses "tool_use_id" and "content" (not "id"/"output")
                                        let tool_use_id = event["tool_use_id"].as_str()
                                            .or_else(|| event["id"].as_str())
                                            .unwrap_or("");
                                        // Extract text from content (array or plain string), output, or result
                                        let output_text = event["content"].as_array()
                                            .and_then(|arr| arr.first())
                                            .and_then(|c| c["text"].as_str())
                                            .or_else(|| event["content"].as_str())
                                            .or_else(|| event["output"].as_str())
                                            .or_else(|| event["result"].as_str())
                                            .map(|s| s.to_string());
                                        let _ = app_handle.emit("ai-tool-result", serde_json::json!({
                                            "id": tool_use_id,
                                            "status": event["status"],
                                            "output": output_text,
                                        }));
                                        // Detect actual build/flash failure from tool result output.
                                        // The output_text may be:
                                        //   1. Raw JSON from MCP tool: {"success": false, "output": "...", "errors": [...]}
                                        //   2. CodeWhale's text summary of the tool result
                                        // We check both JSON "success" field and text patterns for failure.
                                        let is_failure = output_text.as_ref().map_or(false, |text| {
                                            // Try parsing as JSON first
                                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                                                if let Some(ok) = v.get("success").and_then(|s| s.as_bool()) {
                                                    return !ok;
                                                }
                                            }
                                            // Fallback: check text for common failure patterns
                                            let lower = text.to_lowercase();
                                            lower.contains("build failed")
                                                || lower.contains("compilation failed")
                                                || lower.contains("compile error")
                                                || lower.contains("ninja: build stopped")
                                                || lower.contains("error:") && lower.contains("fatal")
                                        });
                                        tracing::info!(
                                            "[AIAssistant] Build result detection: is_failure={}, output_text_len={}",
                                            is_failure,
                                            output_text.as_ref().map(|s| s.len()).unwrap_or(0)
                                        );
                                        let should_emit_done = {
                                            let mut op = ACTIVE_JTAG_OPERATION.lock().await;
                                            let matches = op.as_ref().map_or(false, |o| {
                                                // Empty tool_use_id on the op (legacy / unknown) is
                                                // treated as a wildcard match so older call sites
                                                // still work; when both sides have a real id we
                                                // require an exact match.
                                                o.tool_use_id.is_empty()
                                                    || tool_use_id.is_empty()
                                                    || o.tool_use_id == tool_use_id
                                            });
                                            if matches && op.is_some() {
                                                tracing::info!(
                                                    "[AIAssistant] Clearing ACTIVE_JTAG_OPERATION for tool_use_id={}",
                                                    tool_use_id
                                                );
                                                let final_status = if is_failure { "error" } else { "done" };
                                                if let Some(ref mut active) = *op {
                                                    let had_running = active.steps.iter().any(|s| s.status == "running");
                                                    for s in active.steps.iter_mut() {
                                                        if s.status != "done" && s.status != "error" {
                                                            s.status = final_status.to_string();
                                                        } else if is_failure && s.status == "done" {
                                                            s.status = "error".to_string();
                                                        }
                                                    }
                                                    if had_running {
                                                        tracing::info!(
                                                            "[AIAssistant] Marking all steps as {} on completion",
                                                            final_status
                                                        );
                                                        let _ = app_handle.emit("ai-operation-progress", &*active);
                                                    }
                                                }
                                                *op = None;
                                                true
                                            } else {
                                                tracing::info!(
                                                    "[AIAssistant] tool_result ignored: tool_use_id={}, matches={}",
                                                    tool_use_id,
                                                    matches
                                                );
                                                false
                                            }
                                        };
                                        if should_emit_done {
                                            // Use the same failure detection as above for consistency
                                            let op_status = if is_failure { "error" } else {
                                                event.get("status")
                                                    .and_then(|s| s.as_str())
                                                    .unwrap_or("success")
                                            };
                                            let _ = app_handle.emit("ai-operation-done", serde_json::json!({
                                                "toolUseId": tool_use_id,
                                                "status": op_status,
                                            }));
                                        } else {
                                            info!("tool_result for {} ignored by active JTAG op (ownership mismatch)", tool_use_id);
                                        }
                                        info!("Tool result: id={} output_len={}", tool_use_id,
                                            output_text.as_ref().map(|s| s.len()).unwrap_or(0));

                                        // write_file 完成后立即同步文件变更
                                        if let Some(ref write_path) = pending_write_path.take() {
                                            let has_error = event.get("status")
                                                .and_then(|s| s.as_str())
                                                .map(|s| s.contains("error"))
                                                .unwrap_or(false);
                                            if !has_error {
                                                emit_file_sync_events(&app_handle, project_path.as_deref(), write_path).await;
                                            }
                                        }
                                    }
                                    "session_capture" => {
                                        if let Some(sid) = event["content"].as_str() {
                                            new_session_id = Some(sid.to_string());
                                            info!("CodeWhale session: {}", sid);
                                        }
                                    }
                                    "usage" => {
                                        info!("CodeWhale usage event: {}", serde_json::to_string(&event).unwrap_or_default());
                                        let input = event["input_tokens"].as_u64()
                                            .or_else(|| event["inputTokens"].as_u64())
                                            .or_else(|| event["usage"]["input_tokens"].as_u64())
                                            .or_else(|| event["usage"]["prompt_tokens"].as_u64())
                                            .or_else(|| event["token_usage"]["input_tokens"].as_u64())
                                            .unwrap_or(0);
                                        let output = event["output_tokens"].as_u64()
                                            .or_else(|| event["outputTokens"].as_u64())
                                            .or_else(|| event["usage"]["output_tokens"].as_u64())
                                            .or_else(|| event["usage"]["completion_tokens"].as_u64())
                                            .or_else(|| event["token_usage"]["output_tokens"].as_u64())
                                            .unwrap_or(0);
                                        let cached = event["cached_tokens"].as_u64()
                                            .or_else(|| event["cachedTokens"].as_u64())
                                            .or_else(|| event["usage"]["cached_tokens"].as_u64())
                                            .or_else(|| event["usage"]["prompt_tokens_details"]["cached_tokens"].as_u64())
                                            .or_else(|| event["token_usage"]["cached_tokens"].as_u64())
                                            .unwrap_or(0);
                                        if input > 0 || output > 0 {
                                            msg_input_tokens = input;
                                            msg_output_tokens = output;
                                            msg_cached_tokens = cached;
                                            info!("CodeWhale usage: input={} output={} cached={}", input, output, cached);
                                        }
                                    }
                                    "metadata" => {
                                        info!("CodeWhale metadata event: {}", serde_json::to_string(&event).unwrap_or_default());
                                        if let Some(model) = event["meta"]["model"].as_str() {
                                            info!("CodeWhale model: {}", model);
                                        }
                                        let meta_input = event["meta"]["input_tokens"].as_u64()
                                            .or_else(|| event["meta"]["usage"]["input_tokens"].as_u64())
                                            .or_else(|| event["meta"]["usage"]["prompt_tokens"].as_u64())
                                            .unwrap_or(0);
                                        let meta_output = event["meta"]["output_tokens"].as_u64()
                                            .or_else(|| event["meta"]["usage"]["output_tokens"].as_u64())
                                            .or_else(|| event["meta"]["usage"]["completion_tokens"].as_u64())
                                            .unwrap_or(0);
                                        let meta_cached = event["meta"]["cached_tokens"].as_u64()
                                            .or_else(|| event["meta"]["usage"]["cached_tokens"].as_u64())
                                            .or_else(|| event["meta"]["usage"]["prompt_tokens_details"]["cached_tokens"].as_u64())
                                            .unwrap_or(0);
                                        if meta_input > 0 || meta_output > 0 {
                                            msg_input_tokens = meta_input;
                                            msg_output_tokens = meta_output;
                                            msg_cached_tokens = meta_cached;
                                            info!("CodeWhale metadata usage: input={} output={} cached={}", meta_input, meta_output, meta_cached);
                                        }
                                    }
                                    "done" => {
                                        {
                                            let mut status = AI_STATUS.lock().await;
                                            *status = AIStatus::Idle;
                                        }
                                        if let Some(done_content) = event["content"].as_str() {
                                            output.push_str(done_content);
                                            let _ = app_handle.emit("ai-chunk", done_content);
                                            info!("CodeWhale done (with content, {} chars)", done_content.len());
                                        } else {
                                            info!("CodeWhale done (no content in done event)");
                                        }
                                        // 在 break 前立即发送 usage，避免时序竞争
                                        {
                                            let mut client = AI_CLIENT.lock().await;
                                            client.add_usage(msg_input_tokens, msg_output_tokens, msg_cached_tokens, &current_model);
                                            let last_cost = calculate_cost_rmb(msg_input_tokens, msg_output_tokens, msg_cached_tokens, &current_model);
                                            let emit_usage = AICumulativeUsage {
                                                session: AIUsage {
                                                    input_tokens: client.cumulative_input_tokens,
                                                    output_tokens: client.cumulative_output_tokens,
                                                    cached_tokens: client.cumulative_cached_tokens,
                                                    total_tokens: client.cumulative_input_tokens + client.cumulative_output_tokens,
                                                    cost_rmb: client.cumulative_cost_rmb,
                                                    model: current_model.clone(),
                                                },
                                                last_message: AIUsage {
                                                    input_tokens: msg_input_tokens,
                                                    output_tokens: msg_output_tokens,
                                                    cached_tokens: msg_cached_tokens,
                                                    total_tokens: msg_input_tokens + msg_output_tokens,
                                                    cost_rmb: last_cost,
                                                    model: current_model.clone(),
                                                },
                                                message_count: client.message_count,
                                            };
                                            let _ = app_handle.emit("ai-usage", emit_usage);
                                            info!("AI usage emitted (in done): {} in + {} out + {} cached tokens, ¥{:.6}", msg_input_tokens, msg_output_tokens, msg_cached_tokens, last_cost);
                                        }
                                        break;
                                    }
                                    _ => {
                                        info!(
                                            "CodeWhale unknown event type: '{}' keys=[{}] line={}",
                                            event_type,
                                            event.as_object().map(|o| o.keys().map(|k| k.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default(),
                                            &line[..line.len().min(300)]
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to parse CodeWhale stream JSON: {} - {}",
                                    e,
                                    &line[..line.len().min(200)]
                                );
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("Failed to read stdout: {}", e);
                        break;
                    }
                }
            }
            result = stderr_reader.next_line() => {
                match result {
                    Ok(Some(line)) => {
                        if !line.trim().is_empty() {
                            tracing::warn!("CodeWhale stderr: {}", line);
                            stderr_lines.push(line);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => tracing::error!("Failed to read stderr: {}", e),
                }
            }
        }
    }

    // Flush: give Tauri event system time to deliver all ai-chunk events
    // to the frontend before invoke returns and the listener is unregistered.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    {
        let mut guard = RUNNING_CHILD.lock().await;
        *guard = None;
    }
    // 清理操作状态（会话结束）
    {
        let mut op = ACTIVE_JTAG_OPERATION.lock().await;
        *op = None;
    }

    if let Some(sid) = new_session_id {
        let mut client = AI_CLIENT.lock().await;
        client.session_id = Some(sid);
    }

    if output.is_empty() {
        let mut status = AI_STATUS.lock().await;
        *status = AIStatus::Error;
        if !stderr_lines.is_empty() {
            return Err(format!("CodeWhale error:\n{}", stderr_lines.join("\n")));
        }
        return Err("CodeWhale returned no response. Check the API key and network access.".into());
    }

    {
        let mut status = AI_STATUS.lock().await;
        *status = AIStatus::Idle;
    }

    if let Some(ref pp) = project_path {
        let config_path = std::path::Path::new(pp)
            .join(".espsmith")
            .join("hardware_config.json");
        if config_path.exists() {
            if let Ok(config_str) = std::fs::read_to_string(&config_path) {
                if let Ok(config) =
                    serde_json::from_str::<crate::commands::hardware::HardwareConfig>(
                        &config_str,
                    )
                {
                    let _ = app_handle.emit("hw-config-changed", &config);
                    info!("Emitted hw-config-changed after AI message completed");
                }
            }
        }
    }

    Ok(output)
}

#[tauri::command]
pub async fn ai_get_status() -> Result<AIStatus, String> {
    let status = AI_STATUS.lock().await;
    Ok(status.clone())
}

#[tauri::command]
pub async fn ai_get_usage() -> Result<AICumulativeUsage, String> {
    let client = AI_CLIENT.lock().await;
    let model = client.config.model.clone();
    Ok(AICumulativeUsage {
        session: AIUsage {
            input_tokens: client.cumulative_input_tokens,
            output_tokens: client.cumulative_output_tokens,
            cached_tokens: client.cumulative_cached_tokens,
            total_tokens: client.cumulative_input_tokens + client.cumulative_output_tokens,
            cost_rmb: client.cumulative_cost_rmb,
            model,
        },
        last_message: AIUsage {
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            total_tokens: 0,
            cost_rmb: 0.0,
            model: String::new(),
        },
        message_count: client.message_count,
    })
}

#[tauri::command]
pub async fn ai_reset_usage() -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    client.reset_usage();
    Ok(())
}

#[tauri::command]
pub async fn ai_set_project_path(path: Option<String>) -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    client.config.project_path = path;
    Ok(())
}

#[tauri::command]
pub async fn ai_set_idf_path(path: Option<String>) -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    client.config.idf_path = path;
    Ok(())
}

#[tauri::command]
pub async fn ai_set_target_chip(chip: Option<String>) -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    if client.config.target_chip != chip {
        client.config.chip_changed = true;
    }
    client.config.target_chip = chip;
    Ok(())
}

#[tauri::command]
pub async fn ai_get_target_chip() -> Result<Option<String>, String> {
    let client = AI_CLIENT.lock().await;
    Ok(client.config.target_chip.clone())
}

#[tauri::command]
pub async fn ai_set_flash_port(port: Option<String>) -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    client.config.flash_port = port;
    Ok(())
}

#[tauri::command]
pub async fn ai_get_flash_port() -> Result<Option<String>, String> {
    let client = AI_CLIENT.lock().await;
    Ok(client.config.flash_port.clone())
}

#[tauri::command]
pub async fn ai_set_permission_mode(mode: String) -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    client.config.permission_mode = match mode.as_str() {
        "full" => PermissionMode::Full,
        "ask" => PermissionMode::Ask,
        _ => return Err(format!("i18n:aiBackend.invalidPermissionMode|mode={}", mode)),
    };
    Ok(())
}

#[tauri::command]
pub async fn ai_get_permission_mode() -> Result<String, String> {
    let client = AI_CLIENT.lock().await;
    Ok(match client.config.permission_mode {
        PermissionMode::Full => "full".to_string(),
        PermissionMode::Ask => "ask".to_string(),
    })
}

#[tauri::command]
pub async fn ai_respond_permission(allow: bool) -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    if let Some(tx) = client.permission_response_tx.take() {
        let _ = tx.send(allow);
        client.pending_permission_request = None;
        Ok(())
    } else {
        Err("i18n:aiBackend.noPendingPermission".into())
    }
}

/// 同步设置芯片型号（供 project.rs 在 open_project 时调用）
/// 注意：不设置 chip_changed，因为打开项目时芯片未变化，不需要触发 set-target
pub async fn sync_target_chip(chip: String) {
    let mut client = AI_CLIENT.lock().await;
    client.config.target_chip = Some(chip);
}

/// 同步设置烧录串口（供 project.rs 在 open_project 时调用）
pub async fn sync_flash_port(port: String) {
    let mut client = AI_CLIENT.lock().await;
    client.config.flash_port = Some(port);
}

/// 通知 AI 后端芯片已变更，下次编译时需要执行 set-target
/// 用于新建项目或用户在顶部切换芯片时调用
#[tauri::command]
pub async fn ai_notify_chip_changed() -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    client.config.chip_changed = true;
    Ok(())
}

/// Get the cached IDF path from AI config (used by connection.rs for esptool detection).
/// This uses try_lock() to avoid blocking; if the lock is held, returns None.
pub fn get_cached_idf_path() -> Option<String> {
    AI_CLIENT
        .try_lock()
        .ok()
        .and_then(|client| {
            client.config.idf_path.clone().filter(|p| !p.is_empty())
        })
}


/// 剥离终端转义序列（OSC 标题、ANSI 控制码等），
/// CodeWhale 输出中可能混入 `\x1b]0;...\x07` 等序列，导致 JSON 解析失败。
fn strip_ansi_escapes(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC 序列开始
            i += 1;
            if i < bytes.len() {
                match bytes[i] {
                    b'[' => {
                        // CSI 序列 (e.g. \x1b[0m) — 跳过直到结束字母
                        i += 1;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1; // 跳过结束字节
                        }
                    }
                    b']' => {
                        // OSC 序列 (e.g. \x1b]0;title\x07) — 跳过直到 BEL 或 ST
                        i += 1;
                        while i < bytes.len() && bytes[i] != 0x07 {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1; // 跳过 BEL
                        }
                    }
                    _ => {
                        // 其他 ESC 序列，保守跳过 ESC 和下一个字节
                        if i < bytes.len() {
                            i += 1;
                        }
                    }
                }
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).unwrap_or_else(|_| input.to_string())
}

fn build_hardware_hint(project_path: Option<&str>) -> String {
    match project_path {
        Some(p) if !p.trim().is_empty() => {
            "硬件配置: 用hw_config_add_peripheral添加外设(自动更新hardware_config.json和hardware_pins.h),用get_hardware_config查看当前配置。\
             禁止直接修改hardware_pins.h,该文件由hardware_config.json自动生成。\
             修改硬件引脚请编辑.espsmith/hardware_config.json,保存后hardware_pins.h会自动更新".to_string()
        }
        _ => String::new(),
    }
}

fn sanitize_prompt_for_cmd(prompt: String) -> String {
    prompt
        .replace("\r\n", " ")
        .replace('\n', " ")
        .replace('|', ",")
        .replace('%', "%%")
}

fn build_short_agent_prompt(user_message: &str, project_path: Option<&str>, idf_path: Option<&str>, target_chip: Option<&str>, flash_port: Option<&str>, chip_changed: bool) -> String {
    let project = project_path.unwrap_or("(项目路径)");
    let idf = idf_path.unwrap_or("(IDF路径)");
    let port = flash_port.unwrap_or("(先用list-ports查询)");
    let target_arg = if chip_changed && target_chip.is_some() { format!(" --target {}", target_chip.unwrap()) } else { String::new() };
    let hw_hint = build_hardware_hint(project_path);

    // Resolve espsmith-cli.exe path: same directory as current exe, or in binaries/ subdirectory.
    // In dev mode, espsmith-cli.exe is compiled by beforeDevCommand and placed in target/debug/.
    // Falls back to espsmith.exe itself if espsmith-cli.exe is not found (legacy dev mode).
    let cli_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            let dir = exe.parent()?.to_path_buf();
            // Try same directory first (production: espsmith-cli.exe alongside espsmith.exe;
            // dev: both in target/debug/)
            let same_dir = dir.join("espsmith-cli.exe");
            if same_dir.exists() {
                tracing::info!("[AIAssistant] Found espsmith-cli.exe at {}", same_dir.display());
                return Some(same_dir);
            }
            // Try binaries/ subdirectory (production layout)
            let bin_dir = dir.join("binaries").join("espsmith-cli.exe");
            if bin_dir.exists() {
                tracing::info!("[AIAssistant] Found espsmith-cli.exe at {}", bin_dir.display());
                return Some(bin_dir);
            }
            // Fallback: use espsmith.exe itself (works in legacy dev mode where espsmith-cli isn't compiled)
            tracing::warn!(
                "[AIAssistant] espsmith-cli.exe not found in {} or {}, falling back to espsmith.exe (GUI subsystem — exec_shell may not capture output)",
                dir.display(),
                dir.join("binaries").display()
            );
            Some(exe)
        })
        .map(|p| {
            // 路径可能含空格，用引号包裹
            let s = p.to_string_lossy().to_string();
            if s.contains(' ') {
                format!("\"{}\"", s)
            } else {
                s
            }
        })
        .unwrap_or_else(|| "espsmith-cli.exe".to_string());

    let build_cmd = format!("{cli} build --project \"{project}\" --idf \"{idf}\"{target_arg}", cli=cli_exe, project=project, idf=idf, target_arg=target_arg);
    let flash_cmd = format!("{cli} flash --project \"{project}\" --idf \"{idf}\" --port \"{port}\"", cli=cli_exe, project=project, idf=idf, port=port);
    let monitor_cmd = format!("{cli} monitor --port \"{port}\" --duration 5000", cli=cli_exe, port=port);

    let chip_warn = if target_chip.is_none_or(|c| c == "auto") {
        "芯片型号未配置,请先让用户在工具栏选择。"
    } else { "" };
    let port_warn = if flash_port.is_none_or(|p| p.trim().is_empty()) {
        "烧录串口未配置,烧录前请先让用户在工具栏选择。"
    } else { "" };

    let conn_info = crate::connection::get_cached_connection_info();
    let is_jtag = conn_info.mode.is_jtag();
    let detected_port = conn_info.port.as_deref().unwrap_or(port);

    let civ_context = target_chip
        .and_then(|chip| crate::experience::build_context_prompt(chip, "verify"))
        .map(|ctx| format!("。{}", ctx))
        .unwrap_or_default();

    let direct_rule = if chip_changed {
        "芯片刚切换,编译命令已包含--target参数,直接执行即可(会自动执行set-target重配置)。set-target较慢请耐心等待。若build失败报告错误即可。烧录前确认编译目标与硬件一致。引入第三方组件必须在main/idf_component.yml声明依赖(如espressif/led_strip:\"*\"),同时在main/CMakeLists.txt的idf_component_register中添加REQUIRES组件名,禁止手动下载源码或git submodule,build时自动下载集成。回复简要总结结果即可。重要: build/flash/closed-loop是长时间同步命令(可能需要数分钟),exec_shell执行后必须耐心等待结果返回,绝不要在命令运行中重复执行同一命令或尝试跳过,否则会导致进程冲突和崩溃。如果收到\"Another espsmith command is running\"错误,说明上一次命令仍在运行,必须等待其完成,不要重试。"
    } else {
        "路径已预配置。编译直接执行build命令即可,不要携带--target参数(除非用户明确要求切换芯片,因为set-target会触发完全重配置非常慢)。若build失败报告错误即可。烧录前确认编译目标与硬件一致。引入第三方组件必须在main/idf_component.yml声明依赖(如espressif/led_strip:\"*\"),同时在main/CMakeLists.txt的idf_component_register中添加REQUIRES组件名,禁止手动下载源码或git submodule,build时自动下载集成。回复简要总结结果即可。重要: build/flash/closed-loop是长时间同步命令(可能需要数分钟),exec_shell执行后必须耐心等待结果返回,绝不要在命令运行中重复执行同一命令或尝试跳过,否则会导致进程冲突和崩溃。如果收到\"Another espsmith command is running\"错误,说明上一次命令仍在运行,必须等待其完成,不要重试。"
    };

    let resolved_chip = target_chip
        .map(|s| s.to_string())
        .or_else(|| {
            conn_info.chip_hint.as_ref().and_then(|h| {
                let lower = h.to_ascii_lowercase().replace('-', "");
                if lower == "esp32" { None } else { Some(lower) }
            })
        })
        .unwrap_or_else(|| "esp32".to_string());

    if is_jtag {
        let jtag_label = conn_info.mode_label.as_str();
        let closed_loop_cmd = format!("{cli} closed-loop --project \"{project}\" --idf \"{idf}\" --port \"{port}\"", cli=cli_exe, project=project, idf=idf, port=detected_port);
        let jtag_check_cmd = format!("{cli} jtag-runtime-check --project \"{project}\" --idf \"{idf}\" --port \"{port}\" --chip {chip}", cli=cli_exe, project=project, idf=idf, port=detected_port, chip=resolved_chip);
        let openocd_start_cmd = format!("{cli} openocd-start --chip {chip}", cli=cli_exe, chip=resolved_chip);
        sanitize_prompt_for_cmd(format!(
            "你是ESP32开发者，当前连接模式={jtag_label}(JTAG)，芯片={chip}。文件: list_directory, read_file, write_file。{direct_rule}{chip_warn}{port_warn}{hw_hint}{civ_context}\n编译: exec_shell {build_cmd}\nJTAG闭环验证(首选): exec_shell {closed_loop_cmd}\n  - closed-loop内部自动处理: OpenOCD启动→烧录→串口验证→GDB PC/堆栈验证，一条命令搞定\n  - 如果closed-loop失败，根据错误信息修复代码后重试，不要手动操作OpenOCD/GDB\nJTAG深度检查(仅限设断点/观察变量时): exec_shell {jtag_check_cmd}\n  - 如果jtag-runtime-check失败，回退到closed-loop即可，不要手动连接GDB\n烧录(UART): exec_shell {flash_cmd}\n监控: exec_shell {monitor_cmd}\n端口查询: exec_shell {cli} list-ports\n连接检测: exec_shell {cli} detect-connection\nOpenOCD控制: exec_shell {openocd_start_cmd}, exec_shell {cli} openocd-stop, exec_shell {cli} openocd-is-running\n铁律: 绝对不要直接运行openocd.exe/gdb.exe(会卡死/配置不匹配)，只用espsmith-cli子命令。所有验证优先走closed-loop，不要手动搭建GDB调试链路。\n用户: {msg}",
            build_cmd=build_cmd, closed_loop_cmd=closed_loop_cmd, jtag_check_cmd=jtag_check_cmd, openocd_start_cmd=openocd_start_cmd, flash_cmd=flash_cmd, monitor_cmd=monitor_cmd, cli=cli_exe, chip=resolved_chip, msg=user_message,
        ))
    } else {
        sanitize_prompt_for_cmd(format!(
            "你是ESP32开发者。当前连接模式=UART。文件: list_directory, read_file, write_file。{direct_rule}{chip_warn}{port_warn}{hw_hint}{civ_context}\n编译: exec_shell {build_cmd}\n烧录: exec_shell {flash_cmd}\n监控: exec_shell {monitor_cmd}\n一键闭环: exec_shell {cli} closed-loop --project \"{project}\" --idf \"{idf}\" --port \"{port}\"\n端口查询: exec_shell {cli} list-ports\n连接检测: exec_shell {cli} detect-connection\n用户: {msg}",
            build_cmd=build_cmd, flash_cmd=flash_cmd, monitor_cmd=monitor_cmd, cli=cli_exe, project=project, idf=idf, port=detected_port, msg=user_message,
        ))
    }
}

#[allow(dead_code)] // 安全策略预留
fn is_forbidden_shell_tool(name: &str, input: Option<&serde_json::Value>) -> bool {
    let lower_name = name.to_ascii_lowercase();
    if !matches!(
        lower_name.as_str(),
        "exec_shell" | "shell" | "run_command" | "bash" | "cmd" | "terminal" | "powershell" | "pwsh"
    ) {
        return false;
    }
    let command = input
        .and_then(|v| {
            v.get("command")
                .or_else(|| v.get("cmd"))
                .or_else(|| v.get("args"))
                .and_then(|c| c.as_str())
        })
        .unwrap_or("")
        .to_ascii_lowercase();

    if command.is_empty() {
        return false;
    }

    let current_pid = std::process::id().to_string();
    let lower_cmd = command.to_ascii_lowercase();

    // 禁止任何试图杀死 espsmith 自身进程的命令
    let kill_patterns = [
        "taskkill", "taskkill.exe", "tskill", "stop-process",
        "kill", "pkill", "killall", "wmic", "get-process",
    ];
    let targets_espsmith = lower_cmd.contains("espsmith") || lower_cmd.contains(&current_pid);
    if targets_espsmith {
        for pat in &kill_patterns {
            if lower_cmd.contains(pat) {
                return true;
            }
        }
    }

    if command.contains("espsmith") {
        return false;
    }

    let forbidden_invocations = [
        "idf.py", "export.bat", "export.sh", "install.bat", "install.sh",
        "pip install", "pip3 install",
    ];
    for pat in forbidden_invocations {
        if let Some(pos) = command.find(pat) {
            let before = if pos > 0 { &command[..pos] } else { "" };
            let last_char = before.chars().last();
            if last_char.is_none_or(|c| c == ' ' || c == '&' || c == '|' || c == ';' || c == '\t') {
                return true;
            }
        }
    }

    let dangerous_cmds = [
        "curl ", "wget ",
        "regedit", "reg ",
        "net user", "taskkill", "tasklist",
        "del /", "rmdir /", "rm -", "format ", "diskpart",
        "stop-process", "get-process", "wmic", "tskill",
    ];
    dangerous_cmds.iter().any(|needle| command.contains(needle))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgress {
    pub operation_id: String,
    /// CodeWhale `tool_use.id` for the tool call that produced this op.
    /// Used by `tool_result` to verify ownership before clearing the op,
    /// so a non-JTAG tool result or another op's result cannot steal
    /// `ai-operation-done` from us. Empty string when unknown.
    #[serde(default)]
    pub tool_use_id: String,
    pub operation_type: String,
    pub label: String,
    pub steps: Vec<OperationStep>,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationStep {
    pub label: String,
    pub status: String,
}

fn detect_jtag_operation(command: &str, tool_use_id: &str, is_jtag_mode: bool) -> Option<OperationProgress> {
    let lower = command.to_ascii_lowercase();
    let op_id = format!("op-{}", chrono::Utc::now().timestamp_millis());

    // Strip common shell tails (redirections, backgrounding, pipes) so that
    // `nohup espsmith closed-loop ... 2>&1 | tee log &` is still recognised.
    let normalised = lower
        .split(|c: char| c == '|' || c == ';' || c == '&')
        .map(str::trim_start)
        .find(|seg| !seg.is_empty())
        .unwrap_or("");

    let base = OperationProgress {
        operation_id: op_id,
        tool_use_id: tool_use_id.to_string(),
        operation_type: String::new(),
        label: String::new(),
        steps: Vec::new(),
        command: command.to_string(),
    };

    if normalised.contains("closed-loop") || normalised.contains("closed_loop")
        || lower.contains("closed-loop") || lower.contains("closed_loop")
    {
        let (label, steps) = if is_jtag_mode {
            ("i18n:aiOp.jtagClosedLoop".into(), vec![
                OperationStep { label: "i18n:aiOp.step.checkProjectConfig".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.compileFirmware".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.openocdFlash".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.serialVerify".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.gdbPcStackCheck".into(), status: "pending".into() },
            ])
        } else {
            ("i18n:aiOp.uartClosedLoop".into(), vec![
                OperationStep { label: "i18n:aiOp.step.checkProjectConfig".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.compileFirmware".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.uartFlash".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.serialVerify".into(), status: "pending".into() },
            ])
        };
        Some(OperationProgress {
            operation_type: "closed-loop".into(),
            label,
            steps,
            ..base
        })
    } else if normalised.contains("jtag-runtime-check") || normalised.contains("jtag_runtime_check")
        || lower.contains("jtag-runtime-check") || lower.contains("jtag_runtime_check")
    {
        Some(OperationProgress {
            operation_type: "jtag-runtime-check".into(),
            label: "i18n:aiOp.jtagRuntimeCheck".into(),
            steps: vec![
                OperationStep { label: "i18n:aiOp.step.startOpenOCD".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.connectGDB".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.setBreakpoint".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.runCaptureVars".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.analyzeResults".into(), status: "pending".into() },
            ],
            ..base
        })
    } else if normalised.contains("openocd-start") || normalised.contains("openocd_start")
        || lower.contains("openocd-start") || lower.contains("openocd_start")
    {
        Some(OperationProgress {
            operation_type: "openocd-start".into(),
            label: "i18n:aiOp.startOpenOCD".into(),
            steps: vec![
                OperationStep { label: "i18n:aiOp.step.findOpenOCD".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.configJtagInterface".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.startOpenOCDService".into(), status: "pending".into() },
            ],
            ..base
        })
    } else if (normalised.contains("gdb") || lower.contains("gdb"))
        && (normalised.contains("breakpoint") || normalised.contains("backtrace") || normalised.contains("continue")
            || lower.contains("breakpoint") || lower.contains("backtrace") || lower.contains("continue"))
    {
        Some(OperationProgress {
            operation_type: "gdb-debug".into(),
            label: "i18n:aiOp.gdbDebug".into(),
            steps: vec![
                OperationStep { label: "i18n:aiOp.step.connectGDB".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.execDebugCmd".into(), status: "pending".into() },
                OperationStep { label: "i18n:aiOp.step.readResults".into(), status: "pending".into() },
            ],
            ..base
        })
    } else if normalised.contains("build") || normalised.contains("idf.py build") {
        Some(OperationProgress {
            operation_type: "build".into(),
            label: "i18n:aiOp.compileFirmware".into(),
            steps: vec![
                OperationStep { label: "i18n:aiOp.step.compileFirmware".into(), status: "pending".into() },
            ],
            ..base
        })
    } else if normalised.contains("flash") || normalised.contains("idf.py flash") {
        Some(OperationProgress {
            operation_type: "flash".into(),
            label: "i18n:aiOp.flashFirmware".into(),
            steps: vec![
                OperationStep { label: "i18n:aiOp.step.uartFlash".into(), status: "pending".into() },
            ],
            ..base
        })
    } else {
        None
    }
}

fn is_sensitive_tool(name: &str, input: Option<&serde_json::Value>) -> bool {
    let lower_name = name.to_ascii_lowercase();
    if matches!(lower_name.as_str(), "delete_file" | "remove_file") {
        return true;
    }
    if matches!(
        lower_name.as_str(),
        "exec_shell" | "shell" | "run_command" | "bash" | "cmd" | "terminal" | "powershell" | "pwsh"
    ) {
        let command = input
            .and_then(|v| {
                v.get("command")
                    .or_else(|| v.get("cmd"))
                    .or_else(|| v.get("args"))
                    .and_then(|c| c.as_str())
            })
            .unwrap_or("")
            .to_ascii_lowercase();
        if command.is_empty() {
            return false;
        }
        if command.contains("espsmith") {
            return false;
        }
        let sensitive_patterns = [
            "rm ", "del ", "rmdir", "delete", "format", "diskpart",
            "pip install", "pip3 install",
            "idf.py", "export.bat", "export.sh",
            "curl ", "wget ",
            "regedit", "reg ",
            "net user", "taskkill", "stop-process", "get-process", "wmic", "tskill",
        ];
        for pat in &sensitive_patterns {
            if command.contains(pat) {
                return true;
            }
        }
        return !command.is_empty();
    }
    matches!(lower_name.as_str(), "write_file")
}

fn describe_sensitive_operation(name: &str, input: Option<&serde_json::Value>) -> String {
    let lower_name = name.to_ascii_lowercase();
    if lower_name == "delete_file" || lower_name == "remove_file" {
        let path = input.and_then(|v| v.get("path").and_then(|p| p.as_str())).unwrap_or("i18n:aiBackend.unknown");
        return format!("i18n:aiBackend.deleteFile|path={}", path);
    }
    if lower_name == "write_file" {
        let path = input.and_then(|v| v.get("path").and_then(|p| p.as_str())).unwrap_or("i18n:aiBackend.unknown");
        return format!("i18n:aiBackend.writeFile|path={}", path);
    }
    if matches!(lower_name.as_str(), "exec_shell" | "shell" | "run_command" | "bash" | "cmd" | "terminal" | "powershell" | "pwsh") {
        let cmd = input
            .and_then(|v| v.get("command").or_else(|| v.get("cmd")).or_else(|| v.get("args")))
            .and_then(|c| c.as_str())
            .unwrap_or("i18n:aiBackend.unknownCommand");
        return format!("i18n:aiBackend.execCommand|command={}", if cmd.len() > 80 { &cmd[..80] } else { cmd });
    }
    format!("i18n:aiBackend.operationName|name={}", name)
}

fn ensure_project_agent_instructions(
    project_path: Option<&str>,
    idf_path: Option<&str>,
    ael_path: Option<&str>,
) -> Result<(), String> {
    let project_path = match project_path {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => return Ok(()),
    };
    let agents_path = project_path.join("AGENTS.md");
    let existing = std::fs::read_to_string(&agents_path).unwrap_or_default();
    let start = "<!-- ESPSMITH:START -->";
    let end = "<!-- ESPSMITH:END -->";
    let idf_ver = idf_path.map(|p| crate::idf::get_idf_version(p)).unwrap_or_else(|| "unknown".into());
    let block = format!(
        r#"{start}
# EspSmith 嵌入式闭环工作流

## 环境
- 目标框架: ESP-IDF {idf_ver}
- ESP-IDF 路径: `{idf_path}`（已预配置，无需检查 idf.py 存在性）
- 编译烧录通过 `exec_shell` 调用 `espsmith-cli.exe` 子命令完成（必须用 espsmith-cli.exe 而非 espsmith.exe，因为后者是GUI程序无法输出到管道）
- `list_directory` / `read_file` / `write_file` 仅限项目目录内使用

## 构建/烧录/监控命令（通过 exec_shell 执行）

**工作流**：直接执行 `espsmith-cli.exe build` 编译项目。不要携带 `--target` 参数，因为 set-target 会触发完全重配置非常慢。仅在用户明确要求切换芯片型号时才使用 `--target`。**必须使用 `espsmith-cli.exe`（控制台版本），不要使用 `espsmith.exe`（GUI版本无法输出到管道）。**

| 命令 | 说明 |
|------|------|
| `espsmith-cli.exe build --project <项目路径> --idf <IDF路径> [--target <芯片>]` | 编译项目 |
| `espsmith-cli.exe flash --project <项目路径> --idf <IDF路径> --port <串口>` | 烧录固件 |
| `espsmith-cli.exe monitor --port <串口> [--duration 5000]` | 串口采样 |
| `espsmith-cli.exe list-ports` | 列出可用串口 |
| `espsmith-cli.exe build-flash-monitor --project ... --idf ... --port ... [--target <芯片>]` | 一键:构建→烧录→串口验证 |
| `espsmith-cli.exe closed-loop --project <项目路径> --idf <IDF路径> --port <串口> [--target <芯片>] [--force-jtag] [--force-uart]` | **JTAG/UART 闭环验证**: 构建→烧录→串口验证(自动检测JTAG vs UART，JTAG模式额外进行GDB PC/堆栈验证) |
| `espsmith-cli.exe jtag-runtime-check --project ... --idf ... --port <串口> --chip <芯片> [--breakpoints <函数名>] [--watch-variables <变量名>]` | **JTAG深度运行时检查**: 启动OpenOCD→连接GDB→设断点→运行→捕获变量/寄存器/串口输出 |
| `espsmith-cli.exe openocd-start [--chip <芯片>]` | 启动OpenOCD JTAG服务器(GDB端口3333) |
| `espsmith-cli.exe openocd-stop` | 停止OpenOCD |
| `espsmith-cli.exe openocd-is-running` | 检查OpenOCD是否运行中 |
| `espsmith-cli.exe detect-connection [--port <串口>]` | 检测JTAG vs UART连接模式 |
| `espsmith-cli.exe get-connection-mode` | 获取缓存的连接模式 |

## 闭环工作流（必须按此顺序执行）

1. **检查项目** — 用 `list_directory` / `read_file` 了解现有代码
2. **编辑代码** — 用 `write_file` 修改源文件
3. **构建** — 用 `exec_shell` 执行 `espsmith-cli.exe build ...`
4. **JTAG闭环验证(首选，强烈推荐)** — 构建成功后，用 `exec_shell` 执行:
   - `espsmith-cli.exe closed-loop --project <项目路径> --idf <IDF路径> --port <串口>` 一键构建→烧录→验证
   - JTAG模式下自动进行OpenOCD烧录 + GDB PC/堆栈验证
   - UART模式下自动进行esptool烧录 + 串口验证
   - **closed-loop 内部自动处理所有 OpenOCD/GDB 连接细节，不要手动操作这些工具**
   - 如果 closed-loop 失败，根据错误信息修复代码后重试即可
5. **JTAG深度调试(仅限需要设断点/观察变量时使用)** — 不要作为常规验证手段:
   - `espsmith-cli.exe jtag-runtime-check --project ... --idf ... --port <串口> --chip <芯片> --breakpoints app_main --watch-variables counter`
   - 自动启动OpenOCD、连接GDB、设断点、运行、捕获变量值
   - **如果 jtag-runtime-check 失败，回退到 closed-loop 即可，绝对不要手动连接GDB**
6. **调试** — 如运行异常，检查串口输出/GDB状态定位问题后修复代码重试

## 关键规则
- ESP-IDF 已预配置，无需检查或验证 IDF 路径/工具链，直接构建即可
- 所有操作均通过 `exec_shell` + `espsmith-cli.exe` 子命令完成（不要用 espsmith.exe）
- **禁止**直接调用 idf.py、export.bat、install.bat、pip install 等命令
- **禁止**直接运行 openocd.exe、xtensa-esp-elf-gdb.exe 等底层工具（会因配置不匹配而失败），所有 JTAG/GDB 操作通过 espsmith-cli 子命令完成
- **禁止**安装、修复、删除或重装 ESP-IDF/工具链，IDE 已完成配置
- **禁止**直接修改 `hardware_pins.h`，该文件由 `hardware_config.json` 自动生成。修改硬件引脚请编辑 `.espsmith/hardware_config.json`，保存后自动更新
- **引入第三方组件必须使用 IDF Component Manager（idf_component.yml）**：
  - 在 `main/idf_component.yml` 文件中添加依赖声明（注意：必须放在 `main/` 目录下，不是项目根目录），格式如下：

    ```yaml
    dependencies:
      espressif/组件名: "~版本号"
      # 例如:
      # espressif/led_strip: "*"
      # espressif/button: "^1.0.0"
      # idf-extra-components/dht: "*"
    ```

  - 同时必须在 `main/CMakeLists.txt` 的 `idf_component_register()` 中添加 `REQUIRES 组件名`，否则编译器找不到头文件。例如：
    ```cmake
    idf_component_register(SRCS "main.c" INCLUDE_DIRS "." REQUIRES led_strip)
    ```
  - 添加后执行 `espsmith-cli.exe build --project ... --idf ...` 即可自动下载并集成
  - **禁止**手动下载组件源码放入项目目录、**禁止**使用 git submodule 方式引入组件
  - 常用组件注册表：ESP-IDF 官方组件在 `https://components.espressif.com/`，IDF 额外组件在 `https://github.com/espressif/idf-extra-components`
  - 如果用户要求引入的组件在 Component Registry 中不存在，应告知用户该组件暂不支持自动管理，建议手动放置到 `main/components/` 目录下并在该目录创建 `CMakeLists.txt`
- **必须回复**: 每次工具调用完成后，必须用中文向用户汇报结果
- 失败时根据返回的错误信息修复代码后重试
- 所有文件修改限制在项目目录内
{end}"#,
        idf_ver = idf_ver,
        idf_path = idf_path.unwrap_or("(not configured)")
    );

    // Append AEL multi-instrument closed-loop section if ael_path is configured
    let has_ael = ael_path.map_or(false, |p| !p.trim().is_empty());
    let ael_section = if has_ael {
        format!(
            r#"
<!-- AEL-MULTI-INSTRUMENT:START -->
# AEL 多仪器闭环调试（跨平台验证）

## 当 ESP32 代码涉及外设驱动/协议实现时，应同时使用 AEL 在以下平台验证：

| 平台 | 关键差异 |
|------|---------|
| STM32F103 | ARM Cortex-M3, GPIO/SPI/I2C 寄存器差异 |
| RP2040 | 双核 Cortex-M0+, PIO 状态机 |
| CH32V | RISC-V 架构, WCH 外设寄存器 |
| nRF52840 | ARM Cortex-M4, BLE 协议栈 |
| FPGA (iCE40) | 纯硬件实现, LUT/FF 约束 |

## AEL MCP 工具

| 工具 | 功能 |
|------|------|
| `ael_experience_context(board, test)` | 查询历史运行经验（**运行前必须调用**） |
| `ael_run_test(board, test)` | 运行完整闭环测试（preflight→build→flash→verify） |
| `ael_run_pack(pack, board)` | 运行测试套件 |
| `ael_record_skill(trigger, fix, lesson, scope)` | 记录修复经验（**修复问题后必须调用**） |
| `ael_instrument_doctor(id)` | 检查仪器健康状态 |
| `ael_list_boards` | 列出可用开发板 |
| `ael_list_instruments` | 列出已注册仪器 |

## 跨平台闭环工作流

1. **查询经验** — `ael_experience_context` 获取已知陷阱和修复技能
2. **运行测试** — `ael_run_test` 在其他平台运行相同逻辑的测试
3. **对比结果** — ESP32 vs STM32 vs RP2040 的寄存器/时序差异
4. **修复并记录** — 用 `ael_record_skill` 记录跨平台兼容性修复
5. **重新构建** — 回到 EspSmith 工具链重新构建 ESP32 代码

## 关键规则
- 发现外设驱动 bugs 后，**同时**在 ESP32 和 AEL 平台验证修复
- 跨平台发现的寄存器级差异必须通过 `ael_record_skill` 记录为工程经验
- `ael_experience_context` 应在每次 `ael_run_test` 之前调用
<!-- AEL-MULTI-INSTRUMENT:END -->
"#
        )
    } else {
        String::new()
    };

    let block = format!("{block}{ael_section}");

    let updated = if let (Some(start_idx), Some(end_idx)) = (existing.find(start), existing.find(end)) {
        let after_end = end_idx + end.len();
        format!("{}{}{}", &existing[..start_idx], block, &existing[after_end..])
    } else if existing.trim().is_empty() {
        format!("{block}\n")
    } else {
        format!("{}\n\n{}\n", existing.trim_end(), block)
    };

    std::fs::write(&agents_path, updated).map_err(|e| e.to_string())
}

#[allow(dead_code)] // MCP Server模式预留
fn ensure_codewhale_mcp_server(
    project_path: Option<&str>,
    idf_path: Option<&str>,
    ael_path: Option<&str>,
) -> Result<(), String> {
    let project_path = match project_path {
        Some(path) if !path.trim().is_empty() => path,
        _ => return Ok(()),
    };

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Cannot locate EspSmith executable: {e}"))?;
    let mcp_path = deepseek_mcp_config_path()?;
    if let Some(parent) = mcp_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut root: serde_json::Value = if mcp_path.exists() {
        std::fs::read_to_string(&mcp_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_else(default_mcp_config)
    } else {
        default_mcp_config()
    };

    if let Some(obj) = root.as_object_mut() {
        obj.remove("mcpServers");
    }

    if !root.get("servers").map(|v| v.is_object()).unwrap_or(false) {
        root["servers"] = serde_json::json!({});
    }

    root["servers"]["espsmith"] = serde_json::json!({
        "command": current_exe.to_string_lossy(),
        "args": ["--mcp-server"],
        "env": {
            "ESPSMITH_PROJECT": project_path,
            "ESPSMITH_IDF_PATH": idf_path.unwrap_or("")
        },
        "url": null,
        "connect_timeout": null,
        "execute_timeout": 180,
        "read_timeout": 300,
        "disabled": false,
        "enabled": true,
        "required": true,
        "enabled_tools": [],
        "disabled_tools": []
    });

    // Register AEL MCP Server if ael_path is configured
    if let Some(ael_dir) = ael_path.filter(|v| !v.trim().is_empty()) {
        let ael_script = PathBuf::from(ael_dir).join("ael_mcp_server.py");
        if ael_script.exists() {
            let python = find_python_on_path();
            root["servers"]["ael-embedded-lab"] = serde_json::json!({
                "command": python,
                "args": [ael_script.to_string_lossy()],
                "env": {
                    "AEL_HOME": ael_dir,
                    "PYTHONUNBUFFERED": "1"
                },
                "url": null,
                "connect_timeout": null,
                "execute_timeout": 600,
                "read_timeout": 600,
                "disabled": false,
                "enabled": true,
                "required": false,
                "enabled_tools": [],
                "disabled_tools": []
            });
            info!("Registered AEL MCP server: {}", ael_script.display());
        } else {
            info!("AEL MCP server script not found at: {}", ael_script.display());
        }
    }

    std::fs::write(
        &mcp_path,
        serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[allow(dead_code)] // MCP配置预留
fn default_mcp_config() -> serde_json::Value {
    serde_json::json!({
        "timeouts": {
            "connect_timeout": 10,
            "execute_timeout": 180,
            "read_timeout": 300
        },
        "servers": {}
    })
}

#[allow(dead_code)] // DeepSeek MCP预留
fn deepseek_mcp_config_path() -> Result<PathBuf, String> {
    let home = if cfg!(windows) {
        std::env::var("USERPROFILE").map_err(|_| "USERPROFILE is not set".to_string())?
    } else {
        std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?
    };
    Ok(Path::new(&home).join(".deepseek").join("mcp.json"))
}

#[allow(dead_code)] // ESP-IDF Python检测预留
fn find_python_on_path() -> String {
    // Try python3 first, then python
    for cmd in &["python3", "python"] {
        let check = if cfg!(windows) {
            std::process::Command::new("where")
                .arg(cmd)
                .output()
        } else {
            std::process::Command::new("which")
                .arg(cmd)
                .output()
        };
        if let Ok(out) = check {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .next()
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| cmd.to_string());
                if !path.is_empty() {
                    return path;
                }
            }
        }
    }
    // Fallback
    if cfg!(windows) { "python".to_string() } else { "python3".to_string() }
}

#[tauri::command]
pub async fn ai_set_ael_path(path: Option<String>) -> Result<(), String> {
    let mut client = AI_CLIENT.lock().await;
    client.config.ael_path = path;
    Ok(())
}

// ─── CodeWhale 内置安装 ────────────────────────────────────────────

/// 初始化内嵌 CodeWhale 二进制路径（由 lib.rs 在 setup 时调用）
pub fn init_bundled_codewhale(resource_dir: &Path) {
    // 候选路径：开发模式用 CARGO_MANIFEST_DIR，生产模式用 resource_dir
    let candidates: Vec<PathBuf> = vec![
        // 开发模式：源码目录下的 binaries/
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries"),
        // 生产模式：Tauri 资源目录下的 binaries/
        resource_dir.join("binaries"),
        // 回退：安装器可能将文件平铺到资源根目录
        resource_dir.to_path_buf(),
    ];

    for dir in &candidates {
        let exe = if cfg!(windows) {
            dir.join("codewhale.exe")
        } else {
            dir.join("codewhale")
        };
        if exe.exists() {
            info!("CodeWhale bundled binaries found at: {}", dir.display());
            let _ = BUNDLED_CODEWHALE_DIR.set(dir.clone());
            return;
        }
    }

    info!(
        "CodeWhale bundled binaries not found in any of: {:?}",
        candidates
    );
}

fn get_codewhale_local_dir() -> PathBuf {
    let base = dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("espsmith").join("codewhale")
}

fn get_local_codewhale_binary() -> PathBuf {
    if cfg!(windows) {
        get_codewhale_local_dir().join("codewhale.cmd")
    } else {
        get_codewhale_local_dir().join("bin").join("codewhale")
    }
}

/// 获取内嵌的 CodeWhale 二进制路径
fn get_bundled_codewhale_binary() -> Option<PathBuf> {
    BUNDLED_CODEWHALE_DIR.get().map(|dir| {
        if cfg!(windows) {
            dir.join("codewhale.exe")
        } else {
            dir.join("codewhale")
        }
    })
}

fn has_executable_extension(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(ext.to_ascii_lowercase().as_str(), "exe" | "cmd" | "bat" | "com"),
        None => false,
    }
}

fn which_cmd(cmd: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        let output = std::process::Command::new("where")
            .arg(cmd)
            .output()
            .ok()?;
        if output.status.success() {
            let paths: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| PathBuf::from(s.trim()))
                .filter(|p| p.is_file() && has_executable_extension(p))
                .collect();
            if let Some(path) = paths.into_iter().next() {
                return Some(path);
            }
        }
    } else {
        let output = std::process::Command::new("which")
            .arg(cmd)
            .output()
            .ok()?;
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| Path::new(s).is_file())?;
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// 检查 CodeWhale 安装状态
#[tauri::command]
pub async fn check_codewhale_status() -> Result<String, String> {
    // 优先检查内嵌二进制
    if let Some(bundled) = get_bundled_codewhale_binary() {
        if bundled.exists() {
            return Ok("local".into());
        }
    }

    let local = get_local_codewhale_binary();
    if local.exists() {
        return Ok("local".into());
    }

    if which_cmd("codewhale").is_some() {
        return Ok("system".into());
    }

    Ok("missing".into())
}

/// 自动安装 CodeWhale（内嵌版本直接返回已安装，无需额外操作）
#[tauri::command]
pub async fn setup_codewhale(app_handle: tauri::AppHandle) -> Result<String, String> {
    // 优先使用内嵌二进制
    if let Some(bundled) = get_bundled_codewhale_binary() {
        if bundled.exists() {
            let _ = app_handle.emit("codewhale-setup-progress", "already_installed");
            return Ok("already_installed".into());
        }
    }

    let local_bin = get_local_codewhale_binary();
    if local_bin.exists() {
        let _ = app_handle.emit("codewhale-setup-progress", "already_installed");
        return Ok("already_installed".into());
    }

    // 如果内嵌和本地都没有，尝试通过 npm 安装（向后兼容）
    let _ = app_handle.emit("codewhale-setup-progress", "checking_node");

    let node_exe = which_cmd("node").ok_or_else(|| {
        "i18n:aiBackend.nodejsNotInstalled".to_string()
    })?;

    let _ = app_handle.emit("codewhale-setup-progress", "installing");

    let local_dir = get_codewhale_local_dir();
    fs::create_dir_all(&local_dir).map_err(|e| format!("i18n:aiBackend.createDirFailed|error={}", e))?;

    let npm_bin = node_exe.parent()
        .map(|p| p.join(if cfg!(windows) { "npm.cmd" } else { "npm" }))
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "npm.cmd" } else { "npm" }));

    let status = std::process::Command::new(&npm_bin)
        .args(["install", "--global", "codewhale", "--prefix"])
        .arg(&local_dir)
        .current_dir(&local_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .status()
        .map_err(|e| format!("i18n:aiBackend.npmStartFailed|error={}", e))?;

    if !status.success() {
        return Err("i18n:aiBackend.codewhaleInstallFailed".into());
    }

    if !local_bin.exists() {
        return Err(format!(
            "i18n:aiBackend.installFileNotGenerated|path={}",
            local_dir.display()
        ));
    }

    let _ = app_handle.emit("codewhale-setup-progress", "done");
    info!("CodeWhale installed locally at: {}", local_bin.display());
    Ok("installed".into())
}

/// 确保 CodeWhale 可用 (内部函数，在 ai_send_message 中调用)
fn ensure_codewhale_ready() -> Result<PathBuf, String> {
    // 优先使用内嵌二进制
    if let Some(bundled) = get_bundled_codewhale_binary() {
        if bundled.exists() {
            info!("Using bundled CodeWhale: {}", bundled.display());
            return Ok(bundled);
        }
    }

    let local_bin = get_local_codewhale_binary();
    if local_bin.exists() {
        info!("Using local CodeWhale: {}", local_bin.display());
        return Ok(local_bin);
    }

    for name in &["codewhale", "codewhale.cmd"] {
        if let Some(sys_bin) = which_cmd(name) {
            info!("Using system CodeWhale: {}", sys_bin.display());
            return Ok(sys_bin);
        }
    }

    Err(
        "i18n:aiBackend.codewhaleNotFoundAlt".into()
    )
}