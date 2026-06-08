/**
 * 项目缓存服务
 *
 * 在每个项目根目录下维护 .esp-ai-cache.json，
 * 保存打开过的标签页、AI 聊天记录等状态。
 * 打开项目时自动恢复之前的状态。
 */

import { safeInvoke } from './invoke';
import type { ChatMessage } from '../stores/chatStore';

/** 缓存文件相对于项目根目录的名称 */
const CACHE_FILENAME = '.esp-ai-cache.json';

/** 缓存版本号，用于向前兼容 */
const CACHE_VERSION = 1;

export interface ProjectCacheTab {
    path: string;
}

export interface ProjectCacheData {
    version: number;
    tabs: ProjectCacheTab[];
    activeTabPath: string | null;
    chatMessages: ChatMessage[];
}

/** 构建缓存文件的完整路径 */
function cacheFilePath(projectPath: string): string {
    return `${projectPath.replace(/[\\/]+$/, '')}\\${CACHE_FILENAME}`;
}

/** 从项目目录读取缓存（优先磁盘文件，回退 localStorage） */
export async function loadProjectCache(projectPath: string): Promise<ProjectCacheData | null> {
    try {
        const filePath = cacheFilePath(projectPath);
        const raw = await safeInvoke<string>('read_file', { path: filePath });
        if (raw) {
            const data = JSON.parse(raw) as ProjectCacheData;
            if (data && data.version === CACHE_VERSION) {
                localStorage.setItem(`cache:${filePath}`, raw);
                return data;
            }
        }
    } catch {}

    try {
        const filePath = cacheFilePath(projectPath);
        const cached = localStorage.getItem(`cache:${filePath}`);
        if (cached) {
            const data = JSON.parse(cached) as ProjectCacheData;
            if (data && data.version === CACHE_VERSION) {
                return data;
            }
        }
    } catch {}

    return null;
}

/** 将缓存写入项目目录（同时备份到 localStorage） */
export async function saveProjectCache(
    projectPath: string,
    data: Omit<ProjectCacheData, 'version'>,
): Promise<void> {
    try {
        const filePath = cacheFilePath(projectPath);
        const cacheData: ProjectCacheData = {
            ...data,
            version: CACHE_VERSION,
        };
        const json = JSON.stringify(cacheData, null, 2);
        localStorage.setItem(`cache:${filePath}`, json);
        await safeInvoke('write_file', {
            path: filePath,
            content: json,
            safeMode: false,
        });
    } catch (err) {
        console.error('[ProjectCache] Failed to save cache:', err);
    }
}