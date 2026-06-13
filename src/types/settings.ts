/**
 * 设置相关类型
 */

import { IDFEnvironment } from './hardware';

// 全局应用设置
export interface AppSettings {
  idfPath?: string;
  pythonPath?: string;
  defaultPort?: string;
  defaultProjectPath?: string;
  gdbPort: string;
  openOcdScript?: string;
  useDocker: boolean;
  aiModel: 'deepseek' | 'ollama' | 'mimo';
  deepseekModel: 'deepseek-v4-pro' | 'deepseek-v4-flash';
  mimoModel?: string;
  deepseekApiKey?: string;
  ollamaEndpoint?: string;
  reviewMode: boolean;
}

// 初始默认设置
export const DEFAULT_SETTINGS: AppSettings = {
    idfPath: undefined,
    pythonPath: undefined,
    defaultPort: undefined,
    defaultProjectPath: undefined,
    gdbPort: '3333',
    openOcdScript: undefined,
    useDocker: false,
    aiModel: 'deepseek',
    deepseekModel: 'deepseek-v4-pro',
    mimoModel: 'mimo/mimo-auto',
    deepseekApiKey: undefined,
    ollamaEndpoint: 'http://localhost:11434',
    reviewMode: true,
};

// ESP-IDF检测状态
export interface IDFStatus {
    detected: IDFEnvironment | null;
    userConfigured: IDFEnvironment | null;
    active: IDFEnvironment | null;
}
