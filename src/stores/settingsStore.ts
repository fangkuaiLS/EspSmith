/**
 * 应用设置状态管理
 *
 * 设置通过 localStorage 持久化
 * ESP-IDF 检测/验证仅在 Tauri 环境执行
 * 浏览器模式下保存路径，在主界面使用时做 basic 校验
 */

import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import { AppSettings, DEFAULT_SETTINGS, IDFStatus, IDFEnvironment } from '../types';

export function isTauriEnv(): boolean {
    return typeof window !== 'undefined' &&
        ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}

async function tryInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
    if (!isTauriEnv()) return null;
    try {
        const { invoke } = await import('@tauri-apps/api/core');
        return await invoke<T>(cmd, args);
    } catch {
        return null;
    }
}

interface SettingsStore {
    settings: AppSettings;
    idfStatus: IDFStatus;
    isLoading: boolean;
    idfValidated: boolean;     // 用户是否已手动验证过
    idfValidationError: string | null;

    setSettings: (updates: Partial<AppSettings>) => void;
    detectIDF: () => Promise<void>;
    validateIDFPath: (path: string) => Promise<IDFEnvironment | null>;
    setActiveIDF: (env: IDFEnvironment | null) => void;
    clearIDFValidation: () => void;
}

export const useSettingsStore = create<SettingsStore>()(
    persist(
        (set, get) => ({
            settings: { ...DEFAULT_SETTINGS },
            idfStatus: {
                detected: null,
                userConfigured: null,
                active: null,
            },
            isLoading: false,
            idfValidated: false,
            idfValidationError: null,

            setSettings: (updates: Partial<AppSettings>) => {
                set((state) => ({
                    settings: { ...state.settings, ...updates },
                }));
            },

            detectIDF: async () => {
                if (!isTauriEnv()) {
                    set({ isLoading: false });
                    return;
                }

                set({ isLoading: true });
                try {
                    const detected = await tryInvoke<IDFEnvironment>('idf_detect');
                    const { settings } = get();

                    let userConfigured: IDFEnvironment | null = null;
                    if (settings.idfPath) {
                        userConfigured = await tryInvoke<IDFEnvironment>('idf_validate_path', {
                            path: settings.idfPath,
                        });
                    }

                    const active = userConfigured || detected || null;
                    set({
                        idfStatus: { detected, userConfigured, active },
                        idfValidated: userConfigured !== null,
                        idfValidationError: userConfigured ? null : 'Path validation failed',
                        isLoading: false,
                    });
                } catch {
                    set({ isLoading: false });
                }
            },

            validateIDFPath: async (path: string) => {
                if (!path) {
                    set({ idfValidated: false, idfValidationError: null });
                    return null;
                }

                // 浏览器模式：保存路径，设为已存储但提示需要 Tauri 验证
                if (!isTauriEnv()) {
                    set((state) => ({
                        settings: { ...state.settings, idfPath: path },
                        idfValidated: true,
                        idfValidationError: null,
                        idfStatus: {
                            ...state.idfStatus,
                            userConfigured: {
                                idf_path: path,
                                version: 'saved',
                                tools_path: '',
                                source: 'UserConfigured' as any,
                            },
                            active: {
                                idf_path: path,
                                version: 'saved',
                                tools_path: '',
                                source: 'UserConfigured' as any,
                            },
                        },
                    }));
                    return null;
                }

                // Tauri 环境：真实验证
                set({ isLoading: true, idfValidationError: null });

                // ESP-IDF 路径格式基本校验
                if (!path.includes('esp') && !path.includes('idf')) {
                    set((state) => ({
                        idfValidationError: 'Path does not look like an ESP-IDF directory',
                        isLoading: false,
                        idfValidated: false,
                        idfStatus: {
                            ...state.idfStatus,
                            userConfigured: null,
                            active: state.idfStatus.detected,
                        },
                    }));
                    return null;
                }

                const env = await tryInvoke<IDFEnvironment>('idf_validate_path', { path });
                if (env) {
                    set((state) => ({
                        settings: { ...state.settings, idfPath: path },
                        idfStatus: {
                            ...state.idfStatus,
                            userConfigured: env,
                            active: env,
                        },
                        idfValidated: true,
                        idfValidationError: null,
                        isLoading: false,
                    }));
                } else {
                    set((state) => ({
                        idfValidationError: 'Invalid ESP-IDF directory (missing idf.py or tools/)',
                        isLoading: false,
                        idfValidated: false,
                        idfStatus: {
                            ...state.idfStatus,
                            userConfigured: null,
                            active: state.idfStatus.detected,
                        },
                    }));
                }
                return env;
            },

            setActiveIDF: (env) => {
                set((state) => ({
                    idfStatus: { ...state.idfStatus, active: env },
                }));
            },

            clearIDFValidation: () => {
                set({ idfValidated: false, idfValidationError: null });
            },
        }),
        {
            name: 'espsmith-settings',
            storage: createJSONStorage(() => localStorage),
        }
    )
);