/**
 * 统一的 invoke 包装器
 *
 * 策略：先尝试 Tauri 后端（动态 import @tauri-apps/api/core），
 * 如果不可用则回退到 Mock 文件系统。
 */

import * as MockFS from './fs-mock';

/** 检测是否在 Tauri 环境（Tauri v2 使用 __TAURI_INTERNALS__） */
export function isTauri(): boolean {
    return typeof window !== 'undefined' &&
        ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}

let tauriInvoke: ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>) | null = null;
let tauriReady = false;

async function getTauriInvoke() {
    if (tauriInvoke && tauriReady) return tauriInvoke;
    try {
        const { invoke } = await import('@tauri-apps/api/core');
        tauriInvoke = invoke as (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
        tauriReady = true;
        return tauriInvoke;
    } catch {
        return null;
    }
}

/**
 * 统一调用入口
 *
 * 优先尝试 Tauri IPC，失败则回退到浏览器 Mock。
 * Tauri 模式下自动重试最多 2 次（应对启动时后端未就绪）。
 */
export async function safeInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
    if (isTauri()) {
        const invoke = await getTauriInvoke();
        if (!invoke) {
            console.error(`[invoke:Tauri] Tauri invoke not available for ${cmd}`);
            return mockInvoke<T>(cmd, args);
        }
        const maxRetries = 2;
        for (let attempt = 0; attempt <= maxRetries; attempt++) {
            try {
                return await invoke(cmd, args) as T;
            } catch (err: any) {
                const msg = String(err?.message || err);
                const isConnectionError = msg.includes('ERR_CONNECTION_REFUSED') ||
                    msg.includes('Failed to fetch') ||
                    msg.includes('custom protocol');
                if (isConnectionError && attempt < maxRetries) {
                    await new Promise(r => setTimeout(r, 300 * (attempt + 1)));
                    continue;
                }
                if (isConnectionError) {
                    console.warn(`[invoke:Tauri] IPC unavailable for ${cmd}, falling back to mock`);
                    return mockInvoke<T>(cmd, args);
                }
                console.error(`[invoke:Tauri] ${cmd} failed:`, err);
                throw err;
            }
        }
        return null;
    }

    // 浏览器环境 → Mock 文件系统
    try {
        const result = await mockInvoke<T>(cmd, args);
        return result;
    } catch (err) {
        console.error(`[invoke:Mock] ${cmd} failed:`, err);
        return null;
    }
}

/** 浏览器模式 Mock 命令路由 */
async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
    switch (cmd) {
        case 'create_project': {
            const config = args?.config as any;
            if (!config) throw new Error('Missing config');
            return MockFS.mockCreateProject({
                name: config.name,
                path: config.path,
                chip: config.chip,
                idf_version: config.idf_version || 'v5.1',
                template: config.template || 'empty',
            }) as unknown as T;
        }

        case 'open_project': {
            const path = args?.path as string;
            if (!path) throw new Error('Missing path');
            return MockFS.mockOpenProject(path) as unknown as T;
        }

        case 'load_project_config': {
            const projectPath = args?.projectPath as string;
            if (!projectPath) return { chip: 'ESP32', target: null, flash_port: null } as unknown as T;
            return MockFS.mockLoadProjectConfig(projectPath) as unknown as T;
        }

        case 'save_project_config': {
            const projectPath = args?.projectPath as string;
            const chip = args?.chip as string | undefined;
            const target = args?.target as string | undefined;
            const flashPort = args?.flashPort as string | undefined;
            if (projectPath) {
                MockFS.mockSaveProjectConfig(projectPath, chip, target, flashPort);
            }
            return null;
        }

        case 'list_directory': {
            const path = args?.path as string;
            if (!path) throw new Error('Missing path');
            return MockFS.mockListDirectory(path) as unknown as T;
        }

        case 'read_file': {
            const path = args?.path as string;
            if (!path) throw new Error('Missing path');
            return MockFS.mockReadFile(path) as unknown as T;
        }

        case 'write_file': {
            const path = args?.path as string;
            const content = args?.content as string;
            if (!path) throw new Error('Missing path');
            MockFS.mockWriteFile(path, content || '');
            return null;
        }

        case 'create_file': {
            const parentPath = args?.parentPath as string;
            const name = args?.name as string;
            const content = (args?.content as string) || '';
            if (!parentPath || !name) throw new Error('Missing parentPath or name');
            return MockFS.mockCreateFile(parentPath, name, content) as unknown as T;
        }

        case 'create_folder': {
            const parentPath = args?.parentPath as string;
            const name = args?.name as string;
            if (!parentPath || !name) throw new Error('Missing parentPath or name');
            return MockFS.mockCreateFolder(parentPath, name) as unknown as T;
        }

        case 'rename_file': {
            const oldPath = args?.oldPath as string;
            const newName = args?.newName as string;
            if (!oldPath || !newName) throw new Error('Missing oldPath or newName');
            return MockFS.mockRenameFile(oldPath, newName) as unknown as T;
        }

        case 'delete_file': {
            const path = args?.path as string;
            if (!path) throw new Error('Missing path');
            MockFS.mockDeleteFile(path);
            return null;
        }

        case 'duplicate_file': {
            const path = args?.path as string;
            if (!path) throw new Error('Missing path');
            return MockFS.mockDuplicateFile(path) as unknown as T;
        }

        // 以下命令仅在 Tauri 模式下可用
        case 'get_status':
        case 'start_ai_session':
        case 'commit_ai_changes':
        case 'revert_ai_changes':
        case 'get_hw_config':
        case 'save_hw_config':
        case 'check_pin_conflict':
        case 'export_c_header':
        case 'list_ports':
        case 'list_ports_with_idf':
        case 'idf_get_supported_targets':
        case 'idf_doctor':
        case 'idf_list_templates':
        case 'idf_read_partition_table':
        case 'idf_component_list':
        case 'idf_component_add':
        case 'idf_get_sdkconfig':
        case 'idf_save_sdkconfig': {
            const sdkconfigPath = args?.projectPath as string;
            const configs = (args?.configs as Array<{ key: string; value: string }>) || [];
            if (!sdkconfigPath) throw new Error('Missing projectPath');
            const sdkconfigFile = sdkconfigPath.replace(/\/+$/, '') + '/sdkconfig';
            let content: string;
            try {
                content = MockFS.mockReadFile(sdkconfigFile);
            } catch {
                throw new Error('sdkconfig file not found. Please build the project first.');
            }
            // Build value map — keys from frontend lack "CONFIG_" prefix,
            // but sdkconfig file uses "CONFIG_" prefix, so add both forms.
            const newValues = new Map<string, string>();
            for (const c of configs) {
                newValues.set(c.key, c.value);
                if (!c.key.startsWith('CONFIG_')) {
                    newValues.set(`CONFIG_${c.key}`, c.value);
                }
            }
            // Update in-place, preserve comments
            const lines: string[] = [];
            const found = new Set<string>();
            for (const line of content.split('\n')) {
                const trimmed = line.trim();
                if (!trimmed || trimmed.startsWith('#')) {
                    lines.push(line);
                    continue;
                }
                const eqPos = trimmed.indexOf('=');
                if (eqPos < 0) { lines.push(line); continue; }
                const key = trimmed.substring(0, eqPos).trim();
                const newVal = newValues.get(key);
                if (newVal !== undefined) {
                    found.add(key);
                    const indent = line.substring(0, line.length - line.trimStart().length);
                    lines.push(`${indent}${key}=${newVal}`);
                } else {
                    lines.push(line);
                }
            }
            // Append new keys with CONFIG_ prefix
            for (const [key, value] of newValues) {
                if (key.startsWith('CONFIG_')) continue; // skip duplicate entries
                if (!found.has(key) && !found.has(`CONFIG_${key}`)) {
                    lines.push(`CONFIG_${key}=${value}`);
                }
            }
            MockFS.mockWriteFile(sdkconfigFile, lines.join('\n'));
            console.log(`[Mock] Saved ${newValues.size} sdkconfig values to ${sdkconfigFile}`);
            return null;
        }

        case 'sdkconfig_load':
        case 'idf_add_arduino':
        case 'idf_efuse_summary':
        case 'idf_find_tests':
        case 'create_project_from_template':
        case 'build_project':
        case 'write_and_build':
        case 'flash_project':
        case 'get_build_errors':
        case 'open_serial_port':
        case 'close_serial_port':
        case 'write_serial':
        case 'get_debug_state':
        case 'set_breakpoint':
        case 'continue_execution':
        case 'step_over':
        case 'step_into':
        case 'step_out':
        case 'read_variable':
        case 'analyze_coredump':
        case 'idf_detect':
        case 'idf_build':
        case 'idf_flash':
        case 'idf_monitor':
        case 'idf_set_target':
        case 'ai_start':
        case 'ai_stop':
        case 'ai_send_message':
        case 'ai_get_status':
        case 'ai_set_project_path':
        case 'ai_set_idf_path':
        case 'check_codewhale_status':
            return 'missing' as unknown as T;
        case 'setup_codewhale':
            return 'installed' as unknown as T;
        case 'experience_stats':
            return { skillCount: 0, statCount: 0, path: '' } as unknown as T;
        case 'experience_open_dir':
        case 'experience_export':
        case 'experience_import':
            return null;

        default:
            console.warn(`[invoke] Unknown command in browser mode: ${cmd}`);
            return null;
    }
}