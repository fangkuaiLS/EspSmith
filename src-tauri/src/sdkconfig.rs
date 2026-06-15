//! SDK Configuration via confserver (idf.py confserver).
//!
//! Architecture:
//!   1. Start confserver → get initial values/visibility/ranges/defaults
//!   2. Run kconfig_dump.py → get menu tree structure (titles, help, hierarchy)
//!   3. Merge confserver values into menu tree → return to frontend
//!   4. On value change: send "set" to confserver → get back full update
//!   5. On save: send "save" to confserver → confserver writes sdkconfig file
//!
//! Reference: vscode-esp-idf-extension-master/src/espIdf/menuconfig/

use crate::confserver::ConfserverProcess;
use crate::SdkConfigState;
use crate::sdkconfig_loader::menu_from_kconfig;
use tauri::Emitter;
use tracing::info;

/// Load SDK config: start confserver, get values, merge with menu tree.
#[tauri::command]
pub async fn sdkconfig_load(
    state: tauri::State<'_, SdkConfigState>,
    project_path: String,
    idf_path: String,
) -> Result<serde_json::Value, String> {
    let project_path_clone = project_path.clone();
    let idf_path_clone = idf_path.clone();

    info!("[sdkconfig_load] Starting confserver for {}", project_path);

    // Kill any existing confserver process first (prevents race condition on re-open)
    {
        let mut guard = state.0.lock().map_err(|e| format!("Lock error: {}", e))?;
        if let Some(process) = guard.take() {
            info!("[sdkconfig_load] Killing existing confserver process before starting new one");
            process.kill();
        }
    }

    // Start confserver and get initial values
    info!("[sdkconfig_load] Calling ConfserverProcess::start()...");
    let (process, confserver_values) = tokio::task::spawn_blocking(move || {
        ConfserverProcess::start(&project_path_clone, &idf_path_clone)
    })
    .await
    .map_err(|e| format!("Confserver task panicked: {}", e))?
    .map_err(|e| format!("Failed to start confserver: {}", e))?;
    info!("[sdkconfig_load] Confserver started successfully, got {} values", 
        confserver_values.get("values").and_then(|v| v.as_object()).map_or(0, |o| o.len()));

    // Store process for later use
    {
        let mut guard = state.0.lock().map_err(|e| format!("Lock error: {}", e))?;
        *guard = Some(process);
    }

    // Load menu tree from kconfig (same kconfig_dump.py approach)
    info!("[sdkconfig_load] Loading menu tree from kconfig...");
    let menus = tokio::task::spawn_blocking(move || {
        menu_from_kconfig(&project_path, &idf_path)
    })
    .await
    .map_err(|e| format!("Kconfig menu task panicked: {}", e))?
    .map_err(|e| format!("Failed to load menu tree: {}", e))?;
    info!("[sdkconfig_load] Menu tree loaded");

    // Merge: use confserver for values/visible/ranges/defaults, kconfig_dump for structure
    let result = merge_menu_with_confserver(menus, &confserver_values);

    info!("[sdkconfig_load] Done, menus: {} items", result.get("menus").and_then(|m| m.as_array()).map(|a| a.len()).unwrap_or(0));

    Ok(result)
}

/// Send a "set" command to confserver and return updated values.
#[tauri::command]
pub async fn sdkconfig_set_value(
    state: tauri::State<'_, SdkConfigState>,
    key: String,
    value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Safety net: confserver expects "y"/"n" strings for bool values, not true/false booleans.
    // Convert JSON booleans to strings to match the confserver protocol.
    let value = match &value {
        serde_json::Value::Bool(true) => serde_json::Value::String("y".to_string()),
        serde_json::Value::Bool(false) => serde_json::Value::String("n".to_string()),
        _ => value,
    };
    info!("[sdkconfig_set_value] key={}, value={}", key, value);

    let mut guard = state.0.lock().map_err(|e| format!("Lock error: {}", e))?;
    let process = guard.as_mut().ok_or("confserver not running")?;

    let response = process.set_value(&key, &value)?;
    Ok(response)
}

/// Save configuration to sdkconfig file via confserver.
/// 参考官方 VSCode 插件：直接让 confserver 写入 sdkconfig（无临时文件），
/// 通过 is_saving 标记 + 事件通知前端刷新编辑器显示。
#[tauri::command]
pub async fn sdkconfig_save(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, SdkConfigState>,
    project_path: String,
) -> Result<(), String> {
    let project_path_clean = project_path.trim_end_matches('/').trim_end_matches('\\');
    let sdkconfig_path = format!("{}/sdkconfig", project_path_clean);

    let mut guard = state.0.lock().map_err(|e| format!("Lock error: {}", e))?;
    let process = guard.as_mut().ok_or("confserver not running")?;

    // 直接保存（参考官方插件 saveGuiConfigValues，无临时文件/原子写入）
    process.save(&sdkconfig_path)?;

    // 通知前端关闭并重新打开 sdkconfig，确保编辑器显示最新内容
    let path_str = sdkconfig_path.replace('\\', "/");
    let _ = app_handle.emit("sdkconfig_updated", &path_str);

    info!("[sdkconfig_save] Saved sdkconfig to {}", sdkconfig_path);
    Ok(())
}

/// Close the confserver process.
#[tauri::command]
pub async fn sdkconfig_close(
    state: tauri::State<'_, SdkConfigState>,
) -> Result<(), String> {
    info!("[sdkconfig_close] Closing confserver");
    let mut guard = state.0.lock().map_err(|e| format!("Lock error: {}", e))?;
    if let Some(process) = guard.take() {
        process.kill();
    }
    Ok(())
}

// ---- Menu merging logic ----

use serde_json::Value;

/// Merge confserver values into the menu tree from kconfig_dump.py.
fn merge_menu_with_confserver(menus: Value, confserver_values: &Value) -> Value {
    let mut merged = menus.clone();

    let cs_values = confserver_values.get("values").and_then(|v| v.as_object());
    let cs_visible = confserver_values.get("visible").and_then(|v| v.as_object());
    let cs_ranges = confserver_values.get("ranges").and_then(|v| v.as_object());
    let cs_defaults = confserver_values.get("defaults").and_then(|v| v.as_object());

    if let Some(menu_array) = merged.get_mut("menus").and_then(|m| m.as_array_mut()) {
        let menus_clone = menu_array.clone();
        let merged_array: Vec<Value> = menus_clone.into_iter()
            .map(|m| merge_menu_item(m, cs_values, cs_visible, cs_ranges, cs_defaults))
            .collect();
        *menu_array = merged_array;
    }

    merged
}

fn merge_menu_item(
    mut item: Value,
    cs_values: Option<&serde_json::Map<String, Value>>,
    cs_visible: Option<&serde_json::Map<String, Value>>,
    cs_ranges: Option<&serde_json::Map<String, Value>>,
    cs_defaults: Option<&serde_json::Map<String, Value>>,
) -> Value {
    // Extract owned strings early to avoid borrow conflicts
    let item_type: String = item.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let item_name: String = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Apply visibility from confserver
    if let Some(vs) = cs_visible {
        if let Some(vis) = vs.get(&item_name) {
            item["isVisible"] = vis.clone();
        }
    }

    // Apply value from confserver (for non-menu, non-choice items)
    if item_type != "menu" && item_type != "choice" {
        if let Some(vs) = cs_values {
            if let Some(val) = vs.get(&item_name) {
                item["value"] = val.clone();
            }
        }
    }

    // Apply range
    if let Some(rs) = cs_ranges {
        if let Some(range) = rs.get(&item_name) {
            item["range"] = range.clone();
        }
    }

    // Apply default
    if let Some(ds) = cs_defaults {
        if let Some(def) = ds.get(&item_name) {
            item["default"] = def.clone();
        }
    }

    // Handle choice type: find which child has true value
    if item_type == "choice" {
        if let Some(choices) = item.get_mut("choices").and_then(|c| c.as_array_mut()) {
            let mut selected: Option<String> = None;
            for choice in choices.iter_mut() {
                let cname: String = choice.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(vs) = cs_values {
                    if let Some(val) = vs.get(&cname) {
                        choice["value"] = val.clone();
                        if val.as_bool().unwrap_or(false) || val.as_str() == Some("y") {
                            selected = Some(cname);
                        }
                    }
                }
            }
            if let Some(sel) = selected {
                item["value"] = Value::String(sel);
            }
        }
    }

    // Recurse into items
    if let Some(sub_items) = item.get_mut("items").and_then(|i| i.as_array_mut()) {
        let items_clone = sub_items.clone();
        let merged: Vec<Value> = items_clone.into_iter()
            .map(|i| merge_menu_item(i, cs_values, cs_visible, cs_ranges, cs_defaults))
            .collect();
        *sub_items = merged;
    }

    item
}
