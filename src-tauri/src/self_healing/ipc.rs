//! TCP-based IPC with **delegate mode** for real-time Self-Healing progress.
//!
//! ## Problem
//! When the AI assistant runs `espsmith-cli.exe closed-loop` via `exec_shell`,
//! the CLI is a separate OS process. The old approach tried to forward
//! `RunnerEvent`s from CLI → main process via IPC, but environment variable
//! inheritance through CodeWhale's `exec_shell` was unreliable.
//!
//! ## Solution: Delegate Mode
//! Instead of CLI running the Self-Healing engine and forwarding events, the CLI
//! **delegates** the execution to the main process. The main process runs the
//! engine directly (like UART build/flash), so `RunnerEvent`s are
//! broadcast in-process and reach the Tauri frontend in real-time.
//!
//! Flow:
//! 1. CLI connects to IPC server, sends `{"type":"delegate","command":"closed_loop","args":{...}}`
//! 2. Main process receives delegate request, runs engine via `mcp::call_tool_direct_with_progress`
//! 3. `RunnerEvent`s are broadcast in-process → global listener → `ai-operation-progress` Tauri event
//! 4. Main process sends result back: `{"type":"delegate_result","success":true,"data":{...}}`
//! 5. CLI outputs result to stdout, CodeWhale captures it as `tool_result`
//!
//! Fallback: if IPC is unavailable, CLI runs the engine locally (old behavior).

use crate::self_healing::types::RunnerEvent;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Environment variable name used to pass the IPC address to child processes.
pub const ENV_PIPE_NAME: &str = "ESPSMITH_RUNNER_PIPE";

static SERVER_RUNNING: AtomicBool = AtomicBool::new(false);

/// Derive a TCP port from the main process PID.
/// Port range: 55000–56000 (unlikely to conflict with common services).
fn tcp_port() -> u16 {
    let pid = std::process::id();
    55000 + (pid % 1000) as u16
}

/// Return the TCP address string for the current process.
pub fn pipe_address() -> String {
    format!("127.0.0.1:{}", tcp_port())
}

// ── IPC Message Types ──────────────────────────────────────────────────

/// Messages from CLI → main process.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
enum IpcInbound {
    /// Legacy: forward a RunnerEvent (backward compat for non-delegate path)
    #[serde(rename = "event")]
    Event { data: RunnerEvent },
    /// Delegate: ask main process to run a Self-Healing command
    #[serde(rename = "delegate")]
    Delegate { command: String, args: serde_json::Value },
}

/// Messages from main process → CLI.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
enum IpcOutbound {
    #[serde(rename = "delegate_result")]
    DelegateResult {
        success: bool,
        data: serde_json::Value,
        error: Option<String>,
    },
}

// ── Delegate Handler (registered by lib.rs) ────────────────────────────

type DelegateHandlerFn = dyn Fn(&str, &serde_json::Value) -> DelegateResult + Send + Sync;

/// Result from a delegate execution (mirrors mcp::ToolResult without the import).
pub struct DelegateResult {
    pub success: bool,
    pub data: serde_json::Value,
    pub error: Option<String>,
}

static DELEGATE_HANDLER: Mutex<Option<Box<DelegateHandlerFn>>> = Mutex::new(None);

/// Register the delegate handler. Called once from `lib.rs::run()` setup.
pub fn register_delegate_handler(handler: Box<DelegateHandlerFn>) {
    let mut guard = DELEGATE_HANDLER.lock().unwrap();
    *guard = Some(handler);
    tracing::info!("[IPC] Delegate handler registered");
}

// ── Server side (main espsmith.exe process) ────────────────────────────

/// Start the TCP-based IPC server.
///
/// Listens on `127.0.0.1:{port}` and handles:
/// - `delegate` messages: run Self-Healing engine in main process, return result
/// - `event` messages: re-broadcast RunnerEvent (legacy fallback)
///
/// Call this once during app startup (in `lib.rs::run()`).
pub fn start_ipc_server() {
    if SERVER_RUNNING.load(Ordering::Relaxed) {
        return;
    }

    let addr = pipe_address();
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("[IPC] Failed to bind {}: {}", addr, e);
            return;
        }
    };

    // Set the env var so child processes know where to connect
    std::env::set_var(ENV_PIPE_NAME, &addr);
    SERVER_RUNNING.store(true, Ordering::Relaxed);

    std::thread::spawn(move || {
        tracing::info!("[IPC] Server listening on {}", addr);
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    handle_client(stream);
                }
                Err(e) => {
                    tracing::warn!("[IPC] Accept error: {}", e);
                }
            }
            if !SERVER_RUNNING.load(Ordering::Relaxed) {
                break;
            }
        }
    });
}

fn handle_client(mut stream: TcpStream) {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    tracing::info!("[IPC] Client connected from {}", peer);

    let reader = BufReader::new(&stream);
    for line_result in reader.lines() {
        match line_result {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }
                // Try parsing as IpcInbound first (new protocol)
                match serde_json::from_str::<IpcInbound>(&line) {
                    Ok(IpcInbound::Event { data }) => {
                        tracing::debug!("[IPC] Received event: {:?}", data);
                        crate::self_healing::broadcast_event(&data);
                    }
                    Ok(IpcInbound::Delegate { command, args }) => {
                        tracing::info!("[IPC] Delegate request: command={}", command);
                        let result = run_delegate(&command, &args);
                        // Send result back to CLI
                        let response = IpcOutbound::DelegateResult {
                            success: result.success,
                            data: result.data,
                            error: result.error,
                        };
                        if let Ok(json) = serde_json::to_string(&response) {
                            if let Err(e) = writeln!(&mut stream, "{}", json) {
                                tracing::error!("[IPC] Failed to send delegate result: {}", e);
                            }
                        } else {
                            tracing::error!("[IPC] Failed to serialize delegate result");
                        }
                        // Delegate is one-shot: disconnect after sending result
                        break;
                    }
                    Err(_) => {
                        // Fallback: try parsing as plain RunnerEvent (legacy)
                        match serde_json::from_str::<RunnerEvent>(&line) {
                            Ok(event) => {
                                tracing::debug!("[IPC] Received event (legacy): {:?}", event);
                                crate::self_healing::broadcast_event(&event);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "[IPC] Failed to parse message: {} (line: {})",
                                    e,
                                    &line[..line.len().min(200)]
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!("[IPC] Client disconnected: {}", e);
                break;
            }
        }
    }
    tracing::info!("[IPC] Client {} disconnected", peer);
}

/// Run a delegate request using the registered handler.
fn run_delegate(command: &str, args: &serde_json::Value) -> DelegateResult {
    let guard = DELEGATE_HANDLER.lock().unwrap();
    if let Some(ref handler) = *guard {
        handler(command, args)
    } else {
        DelegateResult {
            success: false,
            data: serde_json::json!({}),
            error: Some("No delegate handler registered".into()),
        }
    }
}

// ── Client side (espsmith-cli.exe child process) ───────────────────────

/// Send a delegate request to the main process and wait for the result.
///
/// Returns `None` if IPC is unavailable (caller should fall back to local execution).
/// Blocks until the main process completes the Self-Healing engine (may take minutes).
pub fn send_delegate_and_wait(command: &str, args: &serde_json::Value) -> Option<DelegateResult> {
    let addr = std::env::var(ENV_PIPE_NAME).ok()?;

    tracing::info!("[IPC] Connecting to parent at {} for delegate: {}", addr, command);
    let mut stream = TcpStream::connect(&addr).ok()?;
    // Self-Healing engine may take minutes; use a long read timeout
    stream.set_read_timeout(Some(std::time::Duration::from_secs(600))).ok();
    stream.set_write_timeout(Some(std::time::Duration::from_millis(2000))).ok();

    // Send delegate request
    let request = IpcInbound::Delegate {
        command: command.to_string(),
        args: args.clone(),
    };
    let json = serde_json::to_string(&request).ok()?;
    writeln!(stream, "{}", json).ok()?;
    tracing::info!("[IPC] Delegate request sent: {}", command);

    // Read result (blocking until main process finishes the Self-Healing engine)
    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        match line {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<IpcOutbound>(&line) {
                    Ok(IpcOutbound::DelegateResult { success, data, error }) => {
                        tracing::info!("[IPC] Delegate result received: success={}", success);
                        return Some(DelegateResult { success, data, error });
                    }
                    Err(e) => {
                        tracing::error!("[IPC] Failed to parse delegate result: {}", e);
                        return None;
                    }
                }
            }
            Err(e) => {
                tracing::error!("[IPC] Error reading delegate result: {}", e);
                return None;
            }
        }
    }
    None
}

/// Send a RunnerEvent to the parent process via IPC (best-effort).
/// Legacy fallback for non-delegate execution path.
pub fn send_event_to_parent(event: &RunnerEvent) {
    let addr = match std::env::var(ENV_PIPE_NAME) {
        Ok(a) => a,
        Err(_) => return,
    };

    // Connect per-event (simple, no cached state)
    if let Ok(mut stream) = TcpStream::connect(&addr) {
        stream.set_write_timeout(Some(std::time::Duration::from_millis(500))).ok();
        let msg = IpcInbound::Event { data: event.clone() };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = writeln!(stream, "{}", json);
        }
    }
}
