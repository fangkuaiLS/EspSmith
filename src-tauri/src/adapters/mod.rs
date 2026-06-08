//! Adapter abstraction layer — inspired by AEL's adapter pattern.
//!
//! Each adapter handles one type of operation (build, flash, verify, GDB)
//! and can be swapped per-board/per-test just like in AEL.
//!
//! Submodules:
//! - `build`:   Build adapters (cmake, idf, zephyr, cargo, etc.)
//! - `flash`:   Flash adapters (gdbmi, esptool, west, uf2, openocd, etc.)
//! - `verify`:  Verify adapters (serial, signal, mailbox, gdb)
//! - `gdb`:     GDB debug adapters

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Normalize a Windows path for GDB/OpenOCD consumption.
///
/// On Windows, `Path::canonicalize()` produces extended-length paths with the
/// `\\?\` prefix (e.g. `\\?\E:\project\build\app.elf`). GDB and OpenOCD do not
/// understand this prefix, and `replace('\\', "/")` turns it into `//?/E:/...`
/// which is also invalid. This function:
/// 1. Strips the `\\?\` prefix if present
/// 2. Converts backslashes to forward slashes
pub fn normalize_path_for_gdb(path: &str) -> String {
    let stripped = if path.starts_with("\\\\?\\") {
        &path[4..]
    } else {
        path
    };
    stripped.replace('\\', "/")
}

pub mod build;
pub mod flash;
pub mod gdb;
pub mod gdb_verify;
pub mod verify;

/// Result of an adapter execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub duration_ms: u64,
    pub error: Option<String>,
}

impl AdapterResult {
    pub fn ok(stdout: Option<String>, duration_ms: u64) -> Self {
        Self {
            success: true,
            exit_code: Some(0),
            stdout,
            stderr: None,
            duration_ms,
            error: None,
        }
    }

    pub fn fail(error: String, stderr: Option<String>, duration_ms: u64) -> Self {
        Self {
            success: false,
            exit_code: Some(1),
            stdout: None,
            stderr,
            duration_ms,
            error: Some(error),
        }
    }
}

/// The Adapter trait — any operation that can be executed.
pub trait Adapter: Send + Sync {
    fn name(&self) -> &str;
    #[allow(dead_code)] // Adapter元数据预留
    fn description(&self) -> &str;
    fn execute(&self, params: &serde_json::Value, work_dir: &str) -> AdapterResult;
}

/// Preflight check adapter — validates serial port availability and board connectivity.
pub struct PreflightCheckAdapter;

impl Adapter for PreflightCheckAdapter {
    fn name(&self) -> &str { "check.preflight" }
    fn description(&self) -> &str { "Pre-flight checks: validate serial port and board" }

    fn execute(&self, params: &serde_json::Value, _work_dir: &str) -> AdapterResult {
        let port = params.get("port").and_then(|v| v.as_str()).unwrap_or("COM3");
        let _board = params.get("board").and_then(|v| v.as_str()).unwrap_or("esp32");
        let start = Instant::now();

        let ports = serialport::available_ports().unwrap_or_default();
        let duration = start.elapsed().as_millis() as u64;

        if ports.iter().any(|p| p.port_name == port) {
            AdapterResult::ok(
                Some(format!("Port {} is available", port)),
                duration,
            )
        } else {
            let available: Vec<String> = ports.iter().map(|p| p.port_name.clone()).collect();
            AdapterResult::fail(
                format!(
                    "Port {} not found. Available: {}",
                    port,
                    if available.is_empty() { "none".into() } else { available.join(", ") }
                ),
                None,
                duration,
            )
        }
    }
}

/// Registry of available adapters, keyed by adapter name.
pub struct AdapterRegistry {
    adapters: HashMap<String, Arc<dyn Adapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self { adapters: HashMap::new() }
    }

    pub fn register(&mut self, adapter: Arc<dyn Adapter>) {
        self.adapters.insert(adapter.name().to_string(), adapter);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Adapter>> {
        self.adapters.get(name).cloned()
    }

    #[allow(dead_code)] // 适配器查询预留
    pub fn has(&self, name: &str) -> bool {
        self.adapters.contains_key(name)
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a registry pre-populated with all built-in IDF adapters.
/// `idf_path` is injected into params for IDF-based adapters.
pub fn create_idf_registry(idf_path: &str) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();

    // Build
    registry.register(Arc::new(build::IdfBuildAdapter));
    // Flash
    registry.register(Arc::new(flash::IdfEsptoolFlashAdapter));
    registry.register(Arc::new(flash::OpenOcdFlashAdapter));
    registry.register(Arc::new(flash::UF2CopyAdapter));
    // Verify
    registry.register(Arc::new(verify::SerialVerifyAdapter));
    registry.register(Arc::new(verify::GdbVerifyAdapter));
    registry.register(Arc::new(gdb_verify::GdbSessionVerifyAdapter));
    // GDB
    registry.register(Arc::new(gdb::GdbDebugAdapter::xtensa()));
    // Preflight
    registry.register(Arc::new(PreflightCheckAdapter));

    // Store idf_path so resolve_adapter can inject it
    let _ = idf_path;
    registry
}

/// Resolve an adapter by name from the registry and execute it.
///
/// For IDF-based adapters (`build.idf`, `flash.idf_esptool`), the `idf_path`
/// is automatically injected into the params.
pub fn resolve_and_execute(
    registry: &AdapterRegistry,
    adapter_name: &str,
    params: &serde_json::Value,
    work_dir: &str,
    idf_path: &str,
) -> AdapterResult {
    let adapter = match registry.get(adapter_name) {
        Some(a) => a,
        None => {
            return AdapterResult::fail(
                format!("Unknown adapter: {}", adapter_name),
                None,
                0,
            );
        }
    };

    let needs_idf = matches!(
        adapter_name,
        "build.idf" | "flash.idf_esptool"
    );

    let effective_params = if needs_idf && !idf_path.is_empty() {
        let mut map = params.clone();
        if let Some(obj) = map.as_object_mut() {
            obj.insert("idf_path".into(), serde_json::Value::String(idf_path.to_string()));
        }
        map
    } else {
        params.clone()
    };

    adapter.execute(&effective_params, work_dir)
}