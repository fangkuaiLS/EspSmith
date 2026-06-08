//! Self-Healing orchestration module — inspired by AEL's pipeline.py.
//!
//! Implements the closed-loop execution model:
//!   plan → preflight → build → flash → verify → report
//!
//! Submodules:
//! - `types`:   Core data structures (Step, Plan, RunResult, RecoveryPolicy)
//! - `stages`:  Self-Healing stage definitions
//! - `runner`:  Retry + rewind + recovery loop
//! - `recovery`: Recovery hints and actions
//!
//! ## 全局事件广播
//!
//! CLI 路径（`espsmith.exe closed-loop`）和 Tauri 命令路径共享同一个广播通道。
//! 自愈引擎执行时产生的 `RunnerEvent`（StepStarted/Passed/Failed/RecoveryApplied）
//! 通过此通道发出，AI 助手线程监听后转发为 Tauri 事件 `ai-runner-event`，
//! 前端据此实时更新操作进度卡片。

pub mod types;
pub mod stages;
pub mod runner;
pub mod recovery;
pub mod ipc;

use crate::self_healing::types::RunnerEvent;
use std::sync::{Arc, Mutex};

/// 全局 RunnerEvent 广播接收器列表。
/// CLI/Tauri 路径注册回调后，自愈引擎执行时每个事件都会通知所有监听者。
static BROADCAST_LISTENERS: Mutex<Vec<Arc<dyn Fn(&RunnerEvent) + Send + Sync>>> = Mutex::new(Vec::new());

/// 注册一个全局事件监听器。返回的 Arc 用于后续移除（drop 即可）。
pub fn add_global_listener(listener: Arc<dyn Fn(&RunnerEvent) + Send + Sync>) {
    let mut listeners = BROADCAST_LISTENERS.lock().unwrap();
    listeners.push(listener);
}

/// 向所有已注册的监听器广播一个 RunnerEvent。
pub fn broadcast_event(event: &RunnerEvent) {
    let listeners = BROADCAST_LISTENERS.lock().unwrap();
    for listener in listeners.iter() {
        listener(event);
    }
}

/// 创建一个通过全局广播通道转发的 RunnerEventSink。
/// 用于 CLI 路径（如 cmd_closed_loop），让自愈引擎事件能到达 Tauri 前端。
#[allow(dead_code)]
pub fn global_sink() -> Arc<dyn Fn(&RunnerEvent) + Send + Sync> {
    Arc::new(|event: &RunnerEvent| {
        broadcast_event(event);
    })
}