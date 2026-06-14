use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::Mutex;
use tauri::Emitter;
use tracing::info;
use crate::connection::ConnectionMode;
use crate::ai_provider::AIProvider;

/// 内嵌的 CodeWhale 二进制目录路径（由 lib.rs 在 setup 时初始化）
static BUNDLED_CODEWHALE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 全局 AppHandle，供 sink 在非 Tauri 命令上下文中 emit 事件
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 初始化全局 AppHandle，在 lib.rs setup 中调用
pub fn init_app_handle(handle: tauri::AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

/// 获取全局 AppHandle 的引用，供 sink 等非 Tauri 命令上下文使用
pub fn app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    #[default]
    Full,
    Ask,
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
    static ref AI_STATUS: Arc<StdMutex<AIStatus>> =
        Arc::new(StdMutex::new(AIStatus::Idle));
    /// 使用 std::sync::Mutex 而非 tokio::sync::Mutex：
    /// 全局监听器在 IPC 线程（非 tokio 线程）中被调用，必须用 lock() 而非 try_lock()。
    /// tokio 任务中也不跨 .await 持锁，所以 std::sync::Mutex 安全。
    static ref ACTIVE_JTAG_OPERATION: Arc<StdMutex<Option<OperationProgress>>> =
        Arc::new(StdMutex::new(None));
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

/// 处理 RunnerEvent 并更新 ACTIVE_JTAG_OPERATION 的步骤状态，然后 emit 到前端。
///
/// 这是进度追踪的核心函数，被以下路径调用：
/// 1. sink（run_delegate_command / mcp_call_tool）—— 主要路径，直接调用
/// 2. 全局监听器（通过 broadcast_event）—— 备用路径，用于 IPC legacy event 等场景
///
/// 关键设计：sink 直接调用此函数而非通过 broadcast_event → 全局监听器间接路径，
/// 避免 ACTIVE_JTAG_OPERATION 未设置时事件被丢弃的时序竞争问题。
pub fn handle_runner_event_for_progress(event: &crate::self_healing::types::RunnerEvent) {
    use crate::self_healing::types::RunnerEvent;

    let ah = match APP_HANDLE.get() {
        Some(h) => h.clone(),
        None => {
            tracing::warn!("[Progress] No APP_HANDLE, dropping event");
            return;
        }
    };

    let mut op = match ACTIVE_JTAG_OPERATION.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let Some(ref mut active) = *op else {
        tracing::warn!("[Progress] Dropping event — no active operation. Event: {:?}", event_summary(event));
        return;
    };

    let step_idx = match event {
        RunnerEvent::StepStarted { step_index, .. } |
        RunnerEvent::StepFailed { step_index, .. } |
        RunnerEvent::StepPassed { step_index, .. } |
        RunnerEvent::RecoveryApplied { step_index, .. } => *step_index,
    };

    if step_idx >= active.steps.len() {
        tracing::warn!(
            "[Progress] step_idx={} >= steps.len()={}, dropping event. operation_type={}, steps={:?}",
            step_idx,
            active.steps.len(),
            active.operation_type,
            active.steps.iter().map(|s| (&s.label, &s.status)).collect::<Vec<_>>()
        );
        return;
    }

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

    tracing::info!(
        "[Progress] Emitting ai-operation-progress: step_idx={}, steps={:?}",
        step_idx,
        active.steps.iter().map(|s| (&s.label, &s.status)).collect::<Vec<_>>()
    );
    let _ = ah.emit("ai-operation-progress", &*active);
}

/// 事件摘要，用于日志输出（避免打印整个事件的大字段）
fn event_summary(event: &crate::self_healing::types::RunnerEvent) -> String {
    use crate::self_healing::types::RunnerEvent;
    match event {
        RunnerEvent::StepStarted { step_index, step_name, attempt, .. } =>
            format!("StepStarted(idx={}, name={}, attempt={})", step_index, step_name, attempt),
        RunnerEvent::StepPassed { step_index, step_name, duration_ms, .. } =>
            format!("StepPassed(idx={}, name={}, dur={}ms)", step_index, step_name, duration_ms),
        RunnerEvent::StepFailed { step_index, step_name, will_retry, .. } =>
            format!("StepFailed(idx={}, name={}, retry={})", step_index, step_name, will_retry),
        RunnerEvent::RecoveryApplied { step_index, action, .. } =>
            format!("RecoveryApplied(idx={}, action={})", step_index, action),
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

    // Ollama 和 MiMo 不需要 API Key
    let needs_api_key = client.config.ai_provider != "ollama" && client.config.ai_provider != "mimo";
    if needs_api_key {
        let _key = client
            .config
            .api_key
            .clone()
            .ok_or_else(|| "Please configure an API Key in Settings first".to_string())?;
    }

    if client.config.enable_tool_use {
        info!("MCP server check skipped — using exec_shell-only mode");
    }

    // 根据 ai_provider 选择对应的 Provider 并检查就绪状态
    let provider = crate::ai_provider::select_provider(&client.config);
    provider.ensure_ready().map_err(|e| {
        format!("i18n:aiBackend.codewhaleNotFound|error={}", e)
    })?;

    Ok(format!("{} Agent is ready", provider.display_name()))
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
                    let _handle = tokio::spawn(crate::commands::hardware::generate_hardware_header(pp.to_string()));
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
    let (model, project_path, idf_path, enable_tool_use, target_chip, flash_port, session_id, _ai_provider, permission_mode, chip_changed) = {
        let client = AI_CLIENT.lock().await;
        // MiMo-Code 免费通道不需要 API Key
        let needs_api_key = client.config.ai_provider != "mimo";
        if needs_api_key {
            let _key = client
                .config
                .api_key
                .clone()
                .ok_or_else(|| "Please configure an API Key in Settings first".to_string())?;
        }
        (
            client.config.model.clone(),
            client.config.project_path.clone(),
            client.config.idf_path.clone(),
            client.config.enable_tool_use,
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
        let mut status = AI_STATUS.lock().unwrap();
        *status = AIStatus::Thinking;
    }


    if enable_tool_use {
        info!("MCP server check skipped — using exec_shell-only mode");
    }

    if enable_tool_use {
        ensure_project_agent_instructions(project_path.as_deref(), idf_path.as_deref())?;
    }

    // 初始化经验库
    if project_path.is_some() {
        let exp_dir = dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("espsmith")
            .join("experience");
        crate::experience::init(exp_dir);
    }

    // 选择 AI Provider（CodeWhale / MiMo-Code）
    let provider = crate::ai_provider::select_provider(&{
        let client = AI_CLIENT.lock().await;
        client.config.clone()
    });
    let provider_name = provider.display_name();

    let binary = provider.ensure_ready().map_err(|e| {
        format!("i18n:aiBackend.codewhaleNotFound|error={}", e)
    })?;

    let prompt = build_short_agent_prompt(&message, project_path.as_deref(), idf_path.as_deref(), target_chip.as_deref(), flash_port.as_deref(), chip_changed);

    // Clear chip_changed flag after it's been consumed by the prompt
    if chip_changed {
        let mut client = AI_CLIENT.lock().await;
        client.config.chip_changed = false;
    }

    let config_snapshot = {
        let client = AI_CLIENT.lock().await;
        client.config.clone()
    };

    let mut provider_cmd = provider.build_command(
        &binary,
        &config_snapshot,
        &prompt,
        session_id.as_deref(),
    );

    // 设置 API Key 环境变量
    if let Some((env_key, env_val)) = provider.api_key_env(&config_snapshot) {
        provider_cmd.cmd.env(&env_key, &env_val);
        info!("Using API key env: {} with model: {}", env_key, model);
    }

    // 显式传递 IPC 管道地址给 AI Provider，确保其 exec_shell 启动的子进程能委托主进程执行闭环
    if let Ok(pipe_addr) = std::env::var(crate::self_healing::ipc::ENV_PIPE_NAME) {
        provider_cmd.cmd.env(crate::self_healing::ipc::ENV_PIPE_NAME, &pipe_addr);
        info!("Passing IPC pipe address to {}: {}", provider_name, pipe_addr);
    }

    if let Some(ref path) = project_path {
        provider_cmd.cmd.current_dir(path);
        info!("{} working directory: {}", provider_name, path);
    }

    info!("Starting {} run (session: {:?})", provider_name, session_id);

    let mut child = provider_cmd.cmd
        .spawn()
        .map_err(|e| format!("i18n:aiBackend.codewhaleStartFailed|path={}|error={}", binary.display(), e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("Failed to read {} stdout", provider_name))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("Failed to read {} stderr", provider_name))?;

    {
        let mut guard = RUNNING_CHILD.lock().await;
        *guard = Some(child);
    }

    // 全局监听器现在只是 handle_runner_event_for_progress 的薄包装，
    // 真正的进度处理逻辑在 handle_runner_event_for_progress 中。
    // sink（run_delegate_command / mcp_call_tool）直接调用该函数，
    // 不再依赖 broadcast_event → 全局监听器这条间接路径。
    {
        static LISTENER_REGISTERED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        let _ = LISTENER_REGISTERED.get_or_init(|| {
            let listener: Arc<dyn Fn(&crate::self_healing::types::RunnerEvent) + Send + Sync> =
                Arc::new(|event| {
                    handle_runner_event_for_progress(event);
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

    // Inactivity timeout: if the subprocess produces no output for this duration,
    // it is likely stuck (e.g. read_file hanging). Kill it and return an error.
    // 300s is generous enough for long-running build/flash operations while still
    // catching genuinely stuck tool calls.
    let inactivity_timeout = Duration::from_secs(300);
    let inactivity_timer = tokio::time::sleep(inactivity_timeout);
    tokio::pin!(inactivity_timer);

    loop {
        tokio::select! {
            result = stdout_reader.next_line() => {
                inactivity_timer.as_mut().reset(tokio::time::Instant::now() + inactivity_timeout);
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
                                // MiMo-Code 事件格式转换：将 MiMo 事件转为 CodeWhale 兼容格式
                                let events_to_process = if provider.kind() == crate::ai_provider::ProviderKind::MiMoCode {
                                    crate::ai_provider::convert_mimo_event(&event)
                                        .unwrap_or_else(|| vec![event])
                                } else {
                                    vec![event]
                                };

                                for event in events_to_process {
                                let event_type = event["type"].as_str().unwrap_or("");
                                match event_type {
                                    "reasoning" => {
                                        // AI 推理/思考过程内容，转发到前端显示
                                        if let Some(text) = event.get("content").and_then(|v| v.as_str())
                                            .or_else(|| event.get("text").and_then(|v| v.as_str()))
                                        {
                                            info!("{} reasoning ({} chars): {}", provider_name, text.len(), &text[..text.char_indices().take(120).last().map(|(i,c)| i+c.len_utf8()).unwrap_or(0)]);
                                            let _ = app_handle.emit("ai-reasoning", text);
                                        }
                                    }
                                    "content" => {
                                        if let Some(content) = event["content"].as_str() {
                                            info!("{} content ({} chars): {}",
                                                provider_name,
                                                content.len(),
                                                &content[..content.char_indices().take(120).last().map(|(i,c)| i+c.len_utf8()).unwrap_or(0)]);
                                            output.push_str(content);
                                            // CodeWhale stream-json: 逐 token 增量
                                            // MiMo-Code --format json: 整段文本完成后一次发出
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
                                        info!("{} tool call: {}", provider_name, name);
                                        // 跟踪 write_file / apply_patch 调用的路径
                                        if name == "write_file" || name == "apply_patch" {
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
                                            let mut status = AI_STATUS.lock().unwrap();
                                            *status = new_status;
                                        }
                                        let tool_use_id_str = extract_id_as_string(&event, "id");
                                        let _ = app_handle.emit("ai-tool-use", serde_json::json!({
                                            "name": name,
                                            "id": tool_use_id_str,
                                            "input": event["input"],
                                        }));
                                        if name == "exec_shell" {
                                            let cmd = event.get("input")
                                                .and_then(|v| v.get("command"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            let tool_use_id = tool_use_id_str.clone();
                                            // 判断当前是否为 JTAG 模式
                                            // 基于用户选择的 flash_port 检测，而非全局缓存
                                            let is_jtag = {
                                                let conn = crate::connection::get_cached_connection_info();
                                                if conn.mode.is_jtag() {
                                                    true
                                                } else if conn.mode != ConnectionMode::Unknown {
                                                    false
                                                } else {
                                                    // Unknown 模式下重新检测
                                                    let flash_port = {
                                                        let c = AI_CLIENT.lock().await;
                                                        c.config.flash_port.clone()
                                                    };
                                                    crate::connection::detect_connection_mode(flash_port.as_deref()).mode.is_jtag()
                                                }
                                            };
                                            tracing::info!(
                                                "[AIAssistant] Connection mode detection: is_jtag={}",
                                                is_jtag
                                            );
                                            if let Some(progress) = detect_jtag_operation(cmd, &tool_use_id, is_jtag) {
                                                tracing::info!(
                                                    "[AIAssistant] Detected operation: type={}, steps={}, tool_use_id={}",
                                                    progress.operation_type,
                                                    progress.steps.len(),
                                                    tool_use_id
                                                );
                                                {
                                                    let mut op = ACTIVE_JTAG_OPERATION.lock().unwrap();
                                                    match *op {
                                                        Some(ref mut existing) => {
                                                            // run_delegate_command 可能已经预设置了 ACTIVE_JTAG_OPERATION，
                                                            // 此时只更新 tool_use_id（用于匹配 tool_result），保留现有进度
                                                            existing.tool_use_id = tool_use_id.to_string();
                                                            tracing::info!(
                                                                "[AIAssistant] Updated tool_use_id for existing operation: id={}",
                                                                tool_use_id
                                                            );
                                                        }
                                                        None => {
                                                            *op = Some(progress.clone());
                                                        }
                                                    }
                                                }
                                                // 发送初始进度卡片（步骤全部 pending）
                                                // 注意：如果 run_delegate_command 已经预设置了，这里会重新 emit，
                                                // 但 operationId 相同，前端会正确更新
                                                let op = ACTIVE_JTAG_OPERATION.lock().unwrap();
                                                if let Some(ref active) = *op {
                                                    let _ = app_handle.emit("ai-operation-progress", active);
                                                }
                                            }
                                        }
                                    }
                                    "tool_result" => {
                                        // CodeWhale uses "tool_use_id" and "content" (not "id"/"output")
                                        // Normalize ID to string for consistent frontend Map key matching
                                        let tool_use_id = {
                                            let id = extract_id_as_string(&event, "tool_use_id");
                                            if id.is_empty() {
                                                extract_id_as_string(&event, "id")
                                            } else {
                                                id
                                            }
                                        };
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
                                        let is_failure = output_text.as_ref().is_some_and(|text| {
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
                                            let mut op = ACTIVE_JTAG_OPERATION.lock().unwrap();
                                            let matches = op.as_ref().is_some_and(|o| {
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
                                            info!("{} session: {}", provider_name, sid);
                                        }
                                    }
                                    "usage" => {
                                        info!("{} usage event: {}", provider_name, serde_json::to_string(&event).unwrap_or_default());
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
                                            info!("{} usage: input={} output={} cached={}", provider_name, input, output, cached);
                                        }
                                    }
                                    "metadata" => {
                                        info!("{} metadata event: {}", provider_name, serde_json::to_string(&event).unwrap_or_default());
                                        if let Some(model) = event["meta"]["model"].as_str() {
                                            info!("{} model: {}", provider_name, model);
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
                                            info!("{} metadata usage: input={} output={} cached={}", provider_name, meta_input, meta_output, meta_cached);
                                        }
                                    }
                                    "step_done" => {
                                        // MiMo-Code 单轮完成（step_finish），不是最终完成
                                        // 更新 session_id，发送 usage
                                        if let Some(sid) = event["session_id"].as_str() {
                                            new_session_id = Some(sid.to_string());
                                            info!("{} step_done, session: {}", provider_name, sid);
                                        }
                                        // 发送当前轮次的 usage
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
                                        }
                                        // 重置当前轮次计数
                                        msg_input_tokens = 0;
                                        msg_output_tokens = 0;
                                        msg_cached_tokens = 0;
                                        // 检查子进程是否已退出，如果已退出则视为完成
                                        {
                                            let mut guard = RUNNING_CHILD.lock().await;
                                            if let Some(ref mut child) = *guard {
                                                match child.try_wait() {
                                                    Ok(Some(_status)) => {
                                                        info!("{} step_done + process exited, treating as final done", provider_name);
                                                        *guard = None;
                                                        drop(guard);
                                                        {
                                                            let mut s = AI_STATUS.lock().unwrap();
                                                            *s = AIStatus::Idle;
                                                        }
                                                        break;
                                                    }
                                                    Ok(None) => {
                                                        // 进程还在运行，继续等待下一轮
                                                        info!("{} step_done but process still running, waiting for next step", provider_name);
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!("{} step_done: try_wait error: {}", provider_name, e);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    "done" => {
                                        {
                                            let mut status = AI_STATUS.lock().unwrap();
                                            *status = AIStatus::Idle;
                                        }
                                        if let Some(done_content) = event["content"].as_str() {
                                            output.push_str(done_content);
                                            let _ = app_handle.emit("ai-chunk", done_content);
                                            info!("{} done (with content, {} chars)", provider_name, done_content.len());
                                        } else {
                                            info!("{} done (no content in done event)", provider_name);
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
                                            "{} unknown event type: '{}' keys=[{}] line={}",
                                            provider_name,
                                            event_type,
                                            event.as_object().map(|o| o.keys().map(|k| k.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default(),
                                            &line[..line.len().min(300)]
                                        );
                                    }
                                }
                                } // for event in events_to_process
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
                inactivity_timer.as_mut().reset(tokio::time::Instant::now() + inactivity_timeout);
                match result {
                    Ok(Some(line)) => {
                        if !line.trim().is_empty() {
                            tracing::warn!("{} stderr: {}", provider_name, line);
                            stderr_lines.push(line);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => tracing::error!("Failed to read stderr: {}", e),
                }
            }
            _ = &mut inactivity_timer => {
                tracing::warn!(
                    "{} inactivity timeout ({}s) — subprocess produced no output, likely stuck. Killing process.",
                    provider_name, inactivity_timeout.as_secs()
                );
                kill_running_child().await;
                let _ = app_handle.emit("ai-chunk", &format!(
                    "\n\n⏱ AI 响应超时（{}秒无输出），已自动终止。这通常是因为工具调用（如 read_file）卡住。请重试或检查文件是否被占用。",
                    inactivity_timeout.as_secs()
                ));
                break;
            }
        }
    }

    // Flush: give Tauri event system time to deliver all ai-chunk events
    // to the frontend before invoke returns and the listener is unregistered.
    tokio::time::sleep(Duration::from_millis(50)).await;

    {
        let mut guard = RUNNING_CHILD.lock().await;
        *guard = None;
    }
    // 清理操作状态（会话结束）
    {
        let mut op = ACTIVE_JTAG_OPERATION.lock().unwrap();
        *op = None;
    }

    if let Some(sid) = new_session_id {
        let mut client = AI_CLIENT.lock().await;
        client.session_id = Some(sid);
    }

    if output.is_empty() {
        let mut status = AI_STATUS.lock().unwrap();
        *status = AIStatus::Error;
        if !stderr_lines.is_empty() {
            return Err(format!("{} error:\n{}", provider_name, stderr_lines.join("\n")));
        }
        return Err(format!("{} returned no response. Check the API key and network access.", provider_name));
    }

    {
        let mut status = AI_STATUS.lock().unwrap();
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
    let status = AI_STATUS.lock().unwrap();
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
    client.config.flash_port = port.clone();
    drop(client); // 释放锁后再检测连接模式
    // 立即基于新端口更新全局连接模式缓存，确保多设备场景下模式与选中端口一致
    crate::connection::detect_connection_mode(port.as_deref());
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
    client.config.flash_port = Some(port.clone());
    drop(client);
    // 立即基于新端口更新全局连接模式缓存
    crate::connection::detect_connection_mode(Some(&port));
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

/// Get the cached flash port from AI config (used by connection.rs for targeted detection).
/// This uses try_lock() to avoid blocking; if the lock is held, returns None.
pub fn get_cached_flash_port() -> Option<String> {
    AI_CLIENT
        .try_lock()
        .ok()
        .and_then(|client| {
            client.config.flash_port.clone().filter(|p| !p.trim().is_empty())
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

/// Extract a JSON value as a string, handling both string and numeric types.
/// CodeWhale may emit tool IDs as numbers (e.g. `"id": 123`) while tool_result
/// always uses strings. JavaScript Map treats `123` and `"123"` as different keys,
/// so we normalize to string here to ensure consistent frontend matching.
fn extract_id_as_string(event: &serde_json::Value, key: &str) -> String {
    event.get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .or_else(|| event.get(key).and_then(|v| v.as_u64().map(|n| n.to_string())))
        .or_else(|| event.get(key).and_then(|v| v.as_i64().map(|n| n.to_string())))
        .unwrap_or_default()
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
    let target_arg = if chip_changed { if let Some(chip) = target_chip { format!(" --target {}", chip) } else { String::new() } } else { String::new() };

    // Resolve espsmith-cli path: same directory as current exe, or in binaries/ subdirectory.
    // In dev mode, espsmith-cli is compiled by beforeDevCommand and placed in target/debug/.
    // Falls back to espsmith itself if espsmith-cli is not found (legacy dev mode).
    let cli_binary_name = if cfg!(windows) { "espsmith-cli.exe" } else { "espsmith-cli" };
    let cli_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            let dir = exe.parent()?.to_path_buf();
            // Try same directory first (production: espsmith-cli alongside espsmith;
            // dev: both in target/debug/)
            let same_dir = dir.join(cli_binary_name);
            if same_dir.exists() {
                tracing::info!("[AIAssistant] Found espsmith-cli at {}", same_dir.display());
                return Some(same_dir);
            }
            // Try binaries/ subdirectory (production layout)
            let bin_dir = dir.join("binaries").join(cli_binary_name);
            if bin_dir.exists() {
                tracing::info!("[AIAssistant] Found espsmith-cli at {}", bin_dir.display());
                return Some(bin_dir);
            }
            // Fallback: use espsmith itself (works in legacy dev mode where espsmith-cli isn't compiled)
            tracing::warn!(
                "[AIAssistant] espsmith-cli not found in {} or {}, falling back to espsmith (GUI subsystem — exec_shell may not capture output)",
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
        .unwrap_or_else(|| cli_binary_name.to_string());

    let build_cmd = format!("{cli} build --project \"{project}\" --idf \"{idf}\"{target_arg}", cli=cli_exe, project=project, idf=idf, target_arg=target_arg);
    let flash_cmd = format!("{cli} flash --project \"{project}\" --idf \"{idf}\" --port \"{port}\"", cli=cli_exe, project=project, idf=idf, port=port);
    let monitor_cmd = format!("{cli} monitor --port \"{port}\" --duration 5000", cli=cli_exe, port=port);

    // 获取 IPC 地址，嵌入到 closed-loop / jtag-runtime-check 命令中
    // 这样即使 CodeWhale 的 exec_shell 不传递环境变量，CLI 也能委托主进程执行
    let ipc_addr_arg = std::env::var(crate::self_healing::ipc::ENV_PIPE_NAME)
        .ok()
        .map(|addr| format!(" --ipc-addr {}", addr))
        .unwrap_or_default();

    let chip_warn = if target_chip.as_ref().map_or(true, |c| *c == "auto") {
        "芯片型号未配置,请先让用户在工具栏选择。"
    } else { "" };
    let port_warn = if flash_port.as_ref().map_or(true, |p| p.trim().is_empty()) {
        "烧录串口未配置,烧录前请先让用户在工具栏选择。"
    } else { "" };

    // 基于用户选择的 flash_port 检测连接模式
    // 优先使用缓存，避免每次请求都枚举串口（50-300ms 开销）
    let conn_info = {
        let cached = crate::connection::get_cached_connection_info();
        if cached.mode != ConnectionMode::Unknown {
            cached
        } else {
            crate::connection::detect_connection_mode(flash_port)
        }
    };
    let is_jtag = conn_info.mode.is_jtag();
    let detected_port = conn_info.port.as_deref().unwrap_or(port);

    let civ_context = target_chip
        .and_then(|chip| crate::experience::build_context_prompt(chip, "verify"))
        .map(|ctx| format!("。{}", ctx))
        .unwrap_or_default();

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
        let closed_loop_cmd = format!("{cli} closed-loop --project \"{project}\" --idf \"{idf}\" --port \"{port}\"{ipc}", cli=cli_exe, project=project, idf=idf, port=detected_port, ipc=ipc_addr_arg);
        let jtag_check_cmd = format!("{cli} jtag-runtime-check --project \"{project}\" --idf \"{idf}\" --port \"{port}\" --chip {chip}{ipc}", cli=cli_exe, project=project, idf=idf, port=detected_port, chip=resolved_chip, ipc=ipc_addr_arg);
        let openocd_start_cmd = format!("{cli} openocd-start --chip {chip}", cli=cli_exe, chip=resolved_chip);
        sanitize_prompt_for_cmd(format!(
            "你是ESP32开发者，连接模式={jtag_label}(JTAG)，芯片={chip}。请先读取AGENTS.md了解工作流规则。{chip_warn}{port_warn}{civ_context}\n编译: exec_shell {build_cmd}\nJTAG闭环验证: exec_shell {closed_loop_cmd}\nJTAG深度检查(仅设断点/观察变量): exec_shell {jtag_check_cmd}\n烧录(UART): exec_shell {flash_cmd}\n监控: exec_shell {monitor_cmd}\n端口查询: exec_shell {cli} list-ports\n连接检测: exec_shell {cli} detect-connection\nOpenOCD: exec_shell {openocd_start_cmd}, exec_shell {cli} openocd-stop, exec_shell {cli} openocd-is-running\n用户: {msg}",
            cli=cli_exe, chip=resolved_chip, msg=user_message,
            build_cmd=build_cmd, closed_loop_cmd=closed_loop_cmd, jtag_check_cmd=jtag_check_cmd,
            openocd_start_cmd=openocd_start_cmd, flash_cmd=flash_cmd, monitor_cmd=monitor_cmd,
        ))
    } else {
        let uart_closed_loop_cmd = format!("{cli} closed-loop --project \"{project}\" --idf \"{idf}\" --port \"{port}\"{ipc}", cli=cli_exe, project=project, idf=idf, port=detected_port, ipc=ipc_addr_arg);
        sanitize_prompt_for_cmd(format!(
            "你是ESP32开发者，连接模式=UART。请先读取AGENTS.md了解工作流规则。{chip_warn}{port_warn}{civ_context}\n编译: exec_shell {build_cmd}\n烧录: exec_shell {flash_cmd}\n监控: exec_shell {monitor_cmd}\n一键闭环: exec_shell {closed_loop_cmd}\n端口查询: exec_shell {cli} list-ports\n连接检测: exec_shell {cli} detect-connection\n用户: {msg}",
            cli=cli_exe, msg=user_message,
            build_cmd=build_cmd, flash_cmd=flash_cmd, monitor_cmd=monitor_cmd, closed_loop_cmd=uart_closed_loop_cmd,
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
            if last_char.map_or(true, |c| c == ' ' || c == '&' || c == '|' || c == ';' || c == '\t') {
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

/// 供委托处理器调用：根据命令类型和连接模式创建 OperationProgress
/// 在 IPC 委托执行前预设置 ACTIVE_JTAG_OPERATION，解决时序竞争
pub fn detect_jtag_operation_for_delegate(command: &str, is_jtag_mode: bool) -> Option<OperationProgress> {
    let fake_cmd = match command {
        "closed_loop" => "closed-loop",
        "jtag_runtime_check" => "jtag-runtime-check",
        _ => command,
    };
    detect_jtag_operation(fake_cmd, "", is_jtag_mode)
}

/// 供 lib.rs 调用：尝试设置 ACTIVE_JTAG_OPERATION（仅在当前为 None 时设置）
/// 返回 true 表示成功设置（之前为 None）
pub fn try_set_active_operation(op: OperationProgress) -> bool {
    let mut active = ACTIVE_JTAG_OPERATION.lock().unwrap();
    if active.is_none() {
        *active = Some(op);
        true
    } else {
        false
    }
}

/// 供 lib.rs 调用：获取 ACTIVE_JTAG_OPERATION 的锁
/// 返回 MutexGuard，调用者可以在持有锁期间读取/修改操作状态
pub fn lock_active_operation() -> std::sync::MutexGuard<'static, Option<OperationProgress>> {
    ACTIVE_JTAG_OPERATION.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn detect_jtag_operation(command: &str, tool_use_id: &str, is_jtag_mode: bool) -> Option<OperationProgress> {
    let lower = command.to_ascii_lowercase();
    let op_id = format!("op-{}", chrono::Utc::now().timestamp_millis());

    // Strip common shell tails (redirections, backgrounding, pipes) so that
    // `nohup espsmith closed-loop ... 2>&1 | tee log &` is still recognised.
    let normalised = lower
        .split(|c| ['|', ';', '&'].contains(&c))
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
    matches!(lower_name.as_str(), "write_file" | "apply_patch")
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
    if lower_name == "apply_patch" {
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
) -> Result<(), String> {
    let project_path = match project_path {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => return Ok(()),
    };
    let agents_path = project_path.join("AGENTS.md");
    let existing = std::fs::read_to_string(&agents_path).unwrap_or_default();
    let start = "<!-- ESPSMITH:START -->";
    let end = "<!-- ESPSMITH:END -->";
    let idf_ver = idf_path.map(crate::idf::get_idf_version).unwrap_or_else(|| "unknown".into());
    let block = format!(
        r#"{start}
# EspSmith 嵌入式闭环工作流

## 环境
- 目标框架: ESP-IDF {idf_ver}
- ESP-IDF 路径: `{idf_path}`（已预配置，无需检查 idf.py 存在性）
- 编译烧录通过 `exec_shell` 调用 `espsmith-cli.exe` 子命令完成（必须用 espsmith-cli.exe 而非 espsmith.exe，因为后者是GUI程序无法输出到管道）
- `list_directory` / `read_file` / `apply_patch` / `write_file` 仅限项目目录内使用
- **优先使用 `apply_patch` 修改文件**：`apply_patch` 接受 unified diff 格式，仅传输变更部分，节省 token 且更安全。仅在创建新文件或重写整个文件时才使用 `write_file`

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
2. **编辑代码** — 优先用 `apply_patch` 修改源文件（仅传变更行，省 token），创建新文件时才用 `write_file`
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
- build/flash/closed-loop 是长时间同步命令（可能需要数分钟），exec_shell 执行后必须耐心等待结果返回，绝不要在命令运行中重复执行同一命令或尝试跳过，否则会导致进程冲突和崩溃
- 如果收到 "Another espsmith command is running" 错误，说明上一次命令仍在运行，必须等待其完成，不要重试
- **修改已有文件必须优先使用 `apply_patch`**（unified diff 格式），避免 `write_file` 全量写入浪费 token。`apply_patch` 格式示例：
  ```
  --- a/main/main.c
  +++ b/main/main.c
  @@ -10,6 +10,8 @@
   #include "freertos/FreeRTOS.h"
   #include "freertos/task.h"

  +#include "driver/gpio.h"
  +
   void app_main(void)
  ```
  仅在创建全新文件时使用 `write_file`
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
  - **优先**通过组件注册表引入组件，避免手动下载组件源码或使用 git submodule 方式
  - 常用组件注册表：ESP-IDF 官方组件在 `https://components.espressif.com/`，IDF 额外组件在 `https://github.com/espressif/idf-extra-components`
  - 如果用户要求引入的组件在 Component Registry 中不存在，应告知用户该组件暂不支持自动管理，建议手动放置到 `main/components/` 目录下并在该目录创建 `CMakeLists.txt`
- **必须回复**: 每次工具调用完成后，必须用中文向用户汇报结果
- 失败时根据返回的错误信息修复代码后重试
- 所有文件修改限制在项目目录内

<!-- EXPERIENCE-ENGINE:START -->
# Experience 经验引擎（AI 疑难杂症积累与自愈）

## 经验数据存储在全局共享目录（所有 ESP32 项目共用），跨项目复用疑难修复经验。
## 只记录真正有价值的疑难问题，不记录常规编译错误、配置拼写等简单问题。

## Experience MCP 工具

| 工具 | 功能 |
|------|------|
| `query_experience(board, test)` | 查询历史疑难经验 |
| `record_experience(trigger, fix, lesson, scope)` | 记录疑难修复经验 |

## 疑难杂症闭环工作流

1. **执行操作** — 编写代码、运行命令、烧录等
2. **遇到疑难** — 非常规错误：API 行为与文档不符、ESP-IDF 已知 bug、硬件时序陷阱等
3. **查询经验** — `query_experience` 查询是否有同类疑难问题的历史修复方案
4. **按经验修复** — 如果经验引擎有记录，优先按历史方案修复；否则自行分析修复
5. **记录经验** — 用 `record_experience` 记录疑难触发条件、修复方法和经验教训
6. **重新构建** — 基于修复结果重新构建和验证

## 什么算疑难杂症（应记录）
- ESP-IDF API 行为与官方文档不一致，需要 workaround
- 特定芯片型号的硬件 errata 或已知 bug
- 外设驱动的时序陷阱（如 SPI/I2C 需要特定延迟才能稳定）
- 内存管理问题（如 DMA 对齐要求、PSRAM 限制）
- Wi-Fi/BLE 协议栈的非常规行为
- 多次尝试才找到根因的隐蔽问题

## 什么不算疑难杂症（不应记录）
- 常规编译错误（缺少头文件、语法错误）
- 配置拼写错误（Kconfig 选项名写错）
- 简单的依赖缺失（未声明 REQUIRES）
- 烧录端口选错等操作失误

## 关键规则
- 遇到疑难问题时，**先调用 `query_experience` 查询历史修复方案**，有记录则优先按经验修复
- 修复疑难问题后，**必须**通过 `record_experience` 记录：触发条件(trigger)、修复方法(fix)、经验教训(lesson)、适用范围(scope)
- `scope` 应填写具体的 ESP32 芯片型号（如 `esp32`、`esp32s3`、`esp32c3`）
- 重复犯相同疑难错误是严重问题，经验引擎的核心目的就是避免此类情况
<!-- EXPERIENCE-ENGINE:END -->
{end}"#,
        idf_ver = idf_ver,
        idf_path = idf_path.unwrap_or("(not configured)")
    );

    let updated = if existing.contains(start) && existing.contains(end) {
        // 替换 ESPSMITH 块（从 START 到 END）
        let start_idx = existing.find(start).unwrap();
        let end_idx = existing.find(end).unwrap();
        let after_end = end_idx + end.len();
        let after_section = &existing[after_end..];

        // 只清理 ESPSMITH:END 之后的残留 EXPERIENCE-ENGINE 块（旧格式遗留）
        // 不清理新 ESPSMITH 块内的 EXPERIENCE-ENGINE（它是正确内容）
        let mut cleaned_after = after_section.to_string();
        while let Some(s) = cleaned_after.find("<!-- EXPERIENCE-ENGINE:START -->") {
            if let Some(e) = cleaned_after[s..].find("<!-- EXPERIENCE-ENGINE:END -->") {
                let after = s + e + "<!-- EXPERIENCE-ENGINE:END -->".len();
                let before = cleaned_after[..s].trim_end_matches('\n').trim_end_matches('\r').to_string();
                let after_str = cleaned_after[after..].trim_start_matches('\n').trim_start_matches('\r').to_string();
                cleaned_after = if after_str.is_empty() {
                    before
                } else {
                    format!("{}\n\n{}", before, after_str)
                };
            } else {
                break;
            }
        }

        format!("{}{}{}", &existing[..start_idx], block, cleaned_after)
    } else if existing.trim().is_empty() {
        format!("{block}\n")
    } else {
        format!("{}\n\n{}\n", existing.trim_end(), block)
    };

    // 仅在内容变更时写入，避免不必要的文件 I/O
    if updated != existing {
        std::fs::write(&agents_path, updated).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[allow(dead_code)] // MCP Server模式预留
fn ensure_codewhale_mcp_server(
    project_path: Option<&str>,
    idf_path: Option<&str>,
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

// ─── Experience 经验库管理 ─────────────────────────────────────────

/// 获取经验库目录路径
fn experience_dir() -> PathBuf {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("espsmith")
        .join("experience")
}

/// 打开经验库所在目录
#[tauri::command]
pub async fn experience_open_dir() -> Result<String, String> {
    let dir = experience_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建经验库目录失败: {e}"))?;
    opener::open(&dir).map_err(|e| format!("打开目录失败: {e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

/// 导出经验库为 JSON 文件
#[tauri::command]
pub async fn experience_export(export_path: String) -> Result<String, String> {
    let dir = experience_dir();
    let skills_dir = dir.join("skills");
    let stats_dir = dir.join("stats");

    let mut skills: Vec<serde_json::Value> = Vec::new();
    if skills_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        skills.push(val);
                    }
                }
            }
        }
    }

    let mut stats: Vec<serde_json::Value> = Vec::new();
    if stats_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&stats_dir) {
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                        stats.push(val);
                    }
                }
            }
        }
    }

    let export_data = serde_json::json!({
        "version": 1,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "skills": skills,
        "stats": stats,
    });

    let content = serde_json::to_string_pretty(&export_data)
        .map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(&export_path, content)
        .map_err(|e| format!("写入文件失败: {e}"))?;

    let count = skills.len();
    Ok(format!("已导出 {count} 条经验到 {}", export_path))
}

/// 从 JSON 文件导入经验库（合并，不覆盖已有记录）
#[tauri::command]
pub async fn experience_import(import_path: String) -> Result<String, String> {
    let content = std::fs::read_to_string(&import_path)
        .map_err(|e| format!("读取文件失败: {e}"))?;
    let data: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析 JSON 失败: {e}"))?;

    let dir = experience_dir();
    let skills_dir = dir.join("skills");
    let stats_dir = dir.join("stats");
    std::fs::create_dir_all(&skills_dir).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&stats_dir).map_err(|e| e.to_string())?;

    // 导入 skills（跳过已存在的）
    let mut imported_skills = 0u32;
    let mut skipped_skills = 0u32;
    if let Some(skills) = data.get("skills").and_then(|v| v.as_array()) {
        for skill in skills {
            if let Some(id) = skill.get("id").and_then(|v| v.as_str()) {
                let dest = skills_dir.join(format!("{id}.json"));
                if dest.exists() {
                    skipped_skills += 1;
                } else {
                    let json = serde_json::to_string_pretty(skill)
                        .map_err(|e| format!("序列化 skill 失败: {e}"))?;
                    std::fs::write(&dest, json)
                        .map_err(|e| format!("写入 skill 失败: {e}"))?;
                    imported_skills += 1;
                }
            }
        }
    }

    // 导入 stats（覆盖合并）
    let mut imported_stats = 0u32;
    if let Some(stats) = data.get("stats").and_then(|v| v.as_array()) {
        for stat in stats {
            // stats 文件名从 board 和 test 字段推导
            let board = stat.get("board").and_then(|v| v.as_str()).unwrap_or("unknown");
            let test = stat.get("test").and_then(|v| v.as_str()).unwrap_or("unknown");
            let dest = stats_dir.join(format!("{board}__{test}.json"));
            let json = serde_json::to_string_pretty(stat)
                .map_err(|e| format!("序列化 stat 失败: {e}"))?;
            std::fs::write(&dest, json)
                .map_err(|e| format!("写入 stat 失败: {e}"))?;
            imported_stats += 1;
        }
    }

    Ok(format!(
        "导入完成: {imported_skills} 条新经验, {skipped_skills} 条已存在跳过, {imported_stats} 条统计记录"
    ))
}

/// 获取经验库统计信息
#[tauri::command]
pub async fn experience_stats() -> Result<serde_json::Value, String> {
    let dir = experience_dir();
    let skills_dir = dir.join("skills");
    let stats_dir = dir.join("stats");

    let skill_count = if skills_dir.exists() {
        std::fs::read_dir(&skills_dir).map(|e| e.count()).unwrap_or(0)
    } else {
        0
    };

    let stat_count = if stats_dir.exists() {
        std::fs::read_dir(&stats_dir).map(|e| e.count()).unwrap_or(0)
    } else {
        0
    };

    Ok(serde_json::json!({
        "skillCount": skill_count,
        "statCount": stat_count,
        "path": dir.to_string_lossy(),
    }))
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

pub fn get_local_codewhale_binary() -> PathBuf {
    if cfg!(windows) {
        get_codewhale_local_dir().join("codewhale.cmd")
    } else {
        get_codewhale_local_dir().join("bin").join("codewhale")
    }
}

/// 获取内嵌的 CodeWhale 二进制路径
pub fn get_bundled_codewhale_binary() -> Option<PathBuf> {
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

pub fn which_cmd(cmd: &str) -> Option<PathBuf> {
    use std::sync::Mutex;
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, Option<PathBuf>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(cmd) {
            return cached.clone();
        }
    }

    let result = _which_cmd_uncached(cmd);

    if let Ok(mut guard) = cache.lock() {
        guard.insert(cmd.to_string(), result.clone());
    }

    result
}

fn _which_cmd_uncached(cmd: &str) -> Option<PathBuf> {
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
            paths.into_iter().next()
        } else {
            None
        }
    } else {
        let output = std::process::Command::new("which")
            .arg(cmd)
            .output()
            .ok()?;
        if output.status.success() {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| Path::new(s).is_file())
                .map(PathBuf::from)
        } else {
            None
        }
    }
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

/// 检查 MiMo-Code 安装状态
#[tauri::command]
pub async fn check_mimo_status() -> Result<String, String> {
    let provider = crate::ai_provider::MiMoCodeProvider;
    Ok(provider.check_status())
}

/// 根据 ai_provider 检查对应 AI 后端的安装状态
#[tauri::command]
pub async fn check_ai_backend_status(ai_provider: String) -> Result<serde_json::Value, String> {
    let kind = crate::ai_provider::ProviderKind::from_str(&ai_provider);
    let (codewhale, mimo) = match kind {
        crate::ai_provider::ProviderKind::CodeWhale => {
            let cw = crate::ai_provider::CodeWhaleProvider.check_status();
            (cw, crate::ai_provider::MiMoCodeProvider.check_status())
        }
        crate::ai_provider::ProviderKind::MiMoCode => {
            let cw = crate::ai_provider::CodeWhaleProvider.check_status();
            let mimo = crate::ai_provider::MiMoCodeProvider.check_status();
            (cw, mimo)
        }
    };
    Ok(serde_json::json!({
        "activeProvider": kind.as_str(),
        "codewhale": codewhale,
        "mimo": mimo,
    }))
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
pub fn ensure_codewhale_ready() -> Result<PathBuf, String> {
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