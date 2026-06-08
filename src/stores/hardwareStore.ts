/**
 * 硬件配置状态管理
 */

import { create } from 'zustand';
import { safeInvoke } from '../lib/invoke';
import type { HardwareConfig, PeripheralInstance, PinConflict, ConnectionInfo, ConnectionMode, IDFEnvironment, PeripheralUpdate } from '../types';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const defaultConnectionInfo: ConnectionInfo = {
  mode: 'unknown',
  modeLabel: 'Undetected',
  recommended: false,
  port: null,
  vid: null,
  pid: null,
  chipHint: null,
  idfTarget: null,
  capabilities: [],
  recommendation: '',
};

interface HardwareState {
  config: HardwareConfig | null;
  idfEnvironment: IDFEnvironment | null;
  peripherals: PeripheralInstance[];
  connectionInfo: ConnectionInfo;
  connectionMode: ConnectionMode;
  conflicts: PinConflict[];
  isLoading: boolean;

  loadConfig: (projectPath: string) => Promise<void>;
  saveConfig: (projectPath: string) => Promise<void>;
  getNextId: (projectPath: string, definitionId: string) => Promise<string>;
  addPeripheral: (projectPath: string, peripheral: PeripheralInstance) => Promise<void>;
  updatePeripheral: (projectPath: string, id: string, update: PeripheralUpdate) => Promise<void>;
  removePeripheral: (projectPath: string, id: string) => Promise<void>;
  checkConflicts: (projectPath: string, newInstance: PeripheralInstance) => Promise<PinConflict[]>;
  exportCHeader: (projectPath: string) => Promise<string>;
  generateHeader: (projectPath: string) => Promise<void>;
  getConfigPrompt: (projectPath: string) => Promise<string>;
  detectConnection: (port?: string) => Promise<void>;
  refreshConnection: (port?: string) => Promise<void>;
}

export const useHardwareStore = create<HardwareState>((set, get) => ({
  config: null,
  idfEnvironment: null,
  peripherals: [],
  conflicts: [],
  isLoading: false,
  connectionInfo: defaultConnectionInfo,
  connectionMode: 'unknown',

  loadConfig: async (projectPath) => {
    set({ isLoading: true });
    try {
      const config = await safeInvoke<HardwareConfig>('get_hw_config', { projectPath });
      set({ config, isLoading: false });
    } catch (err) {
      console.warn('[HardwareStore] Failed to load config:', err);
      set({ isLoading: false });
    }
  },

  saveConfig: async (projectPath) => {
    const { config } = get();
    if (!config) return;
    try {
      await safeInvoke('save_hw_config', { projectPath, config });
    } catch (error) {
      console.error('Failed to save hardware config:', error);
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `保存硬件配置失败: ${error instanceof Error ? error.message : String(error)}`);
      } catch { /* toast not available */ }
    }
  },

  getNextId: async (projectPath, definitionId) => {
    try {
      return await safeInvoke<string>('hw_config_get_next_id', { projectPath, definitionId }) || `${definitionId}_${Date.now()}`;
    } catch (err) {
      console.warn('[HardwareStore] Failed to get next id:', err);
      return `${definitionId}_${Date.now()}`;
    }
  },

  addPeripheral: async (projectPath, peripheral) => {
    try {
      const config = await safeInvoke<HardwareConfig>('hw_config_add_peripheral', {
        projectPath,
        peripheral,
      });
      set({ config });
    } catch (error) {
      console.error('Failed to add peripheral:', error);
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `添加外设失败: ${error instanceof Error ? error.message : String(error)}`);
      } catch { }
      throw error;
    }
  },

  updatePeripheral: async (projectPath, id, update) => {
    try {
      const config = await safeInvoke<HardwareConfig>('hw_config_update_peripheral', {
        projectPath,
        id,
        update,
      });
      set({ config });
    } catch (error) {
      console.error('Failed to update peripheral:', error);
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `更新外设失败: ${error instanceof Error ? error.message : String(error)}`);
      } catch { }
      throw error;
    }
  },

  removePeripheral: async (projectPath, id) => {
    try {
      const config = await safeInvoke<HardwareConfig>('hw_config_remove_peripheral', {
        projectPath,
        id,
      });
      set({ config });
    } catch (error) {
      console.error('Failed to remove peripheral:', error);
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `删除外设失败: ${error instanceof Error ? error.message : String(error)}`);
      } catch { }
    }
  },

  checkConflicts: async (projectPath, newInstance) => {
    try {
      const conflicts = await safeInvoke<PinConflict[]>('check_pin_conflict', {
        projectPath,
        newInstance,
      });
      set({ conflicts: conflicts || [] });
      return conflicts || [];
    } catch (err) {
      console.warn('[HardwareStore] Failed to check conflicts:', err);
      return [];
    }
  },

  exportCHeader: async (projectPath) => {
    try {
      return await safeInvoke<string>('export_c_header', { projectPath }) || '';
    } catch (err) {
      console.warn('[HardwareStore] Failed to export C header:', err);
      return '';
    }
  },

  generateHeader: async (projectPath) => {
    try {
      await invoke('generate_hardware_header', { projectPath });
    } catch (err) {
      console.warn('[HardwareStore] Failed to generate hardware header:', err);
    }
  },

  getConfigPrompt: async (projectPath) => {
    try {
      return await safeInvoke<string>('hw_config_to_prompt', { projectPath }) || '';
    } catch (err) {
      console.warn('[HardwareStore] Failed to get config prompt:', err);
      return '';
    }
  },

  detectConnection: async (port?: string) => {
    try {
      const info = await invoke<ConnectionInfo>('detect_connection', { port: port ?? null });
      console.log('[HardwareStore] detectConnection result:', info.mode, info.port, `idfTarget=${info.idfTarget}`);
      set({ connectionInfo: info, connectionMode: info.mode });
      // 通知 App 组件进行自动选择（比 useEffect 更可靠）
      window.dispatchEvent(new CustomEvent('esp-device-detected', { detail: info }));
    } catch (err) {
      console.warn('[HardwareStore] Failed to detect connection:', err);
    }
  },

  refreshConnection: async (port?: string) => {
    try {
      const info = await invoke<ConnectionInfo>('force_refresh_connection', { port: port ?? null });
      set({ connectionInfo: info, connectionMode: info.mode });
    } catch (err) {
      console.warn('[HardwareStore] Failed to refresh connection:', err);
    }
  },
}));

listen<ConnectionInfo>('connection_changed', (event) => {
  const info = event.payload;
  console.log(
    '[HardwareStore] connection_changed event:',
    info.mode,
    info.chipHint ? `(${info.chipHint})` : '',
    info.port ? `on ${info.port}` : '',
    `idfTarget=${info.idfTarget}`
  );
  useHardwareStore.setState({
    connectionInfo: info,
    connectionMode: info.mode,
  });
  // 通知 App 组件进行自动选择（比 useEffect 更可靠）
  window.dispatchEvent(new CustomEvent('esp-device-detected', { detail: info }));
}).then(() => {
  console.log('[HardwareStore] Port watcher listener registered');
}).catch((err) => {
  console.warn('[HardwareStore] Failed to register port watcher listener:', err);
});

listen<HardwareConfig>('hw-config-changed', (event) => {
  console.log('[HardwareStore] Hardware config changed by AI:', Object.keys(event.payload.peripherals).length, 'peripherals');
  useHardwareStore.setState({ config: event.payload });
}).catch((err) => {
  console.warn('[HardwareStore] Failed to register hw-config-changed listener:', err);
});