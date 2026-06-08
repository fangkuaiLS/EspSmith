/**
 * 项目状态管理
 */

import { create } from 'zustand';
import { safeInvoke } from '../lib/invoke';
import { loadProjectCache, saveProjectCache } from '../lib/projectCache';
import { useFileStore } from './fileStore';
import { useChatStore } from './chatStore';
import type { ProjectInfo } from '../types';

interface ProjectState {
  currentProject: ProjectInfo | null;
  isLoading: boolean;
  error: string | null;

  // Actions
  createProject: (name: string, path: string, chip: string, idfPath: string) => Promise<void>;
  openProject: (path: string) => Promise<void>;
  closeProject: () => void;
  /** 保存当前项目缓存（标签页 + 聊天记录） */
  saveCurrentCache: () => Promise<void>;
  /** 恢复当前项目缓存 */
  restoreCurrentCache: () => Promise<void>;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  currentProject: null,
  isLoading: false,
  error: null,

  createProject: async (name, path, chip, idfPath) => {
    set({ isLoading: true, error: null });
    try {
      // 先保存旧项目缓存
      await get().saveCurrentCache();
      await safeInvoke('create_project', {
        config: { name, path, chip, idf_path: idfPath },
      });
      const projectPath = `${path}\\${name}`;
      const project = await safeInvoke<ProjectInfo>('open_project', { path: projectPath });
      set({ currentProject: project, isLoading: false });
    } catch (error) {
      set({ isLoading: false, error: String(error) });
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `创建项目失败: ${error instanceof Error ? error.message : String(error)}`);
      } catch { /* toast not available */ }
    }
  },

  openProject: async (path) => {
    set({ isLoading: true, error: null });
    try {
      console.log('[ProjectStore] openProject start:', path);
      await get().saveCurrentCache();
      console.log('[ProjectStore] calling open_project IPC...');
      const project = await safeInvoke<ProjectInfo>('open_project', { path });
      console.log('[ProjectStore] open_project result:', project);
      if (!project) {
        set({ isLoading: false, error: `Failed to open project: ${path}` });
        return;
      }
      set({ currentProject: project, isLoading: false });
      console.log('[ProjectStore] openProject done');
    } catch (error) {
      console.error('[ProjectStore] openProject error:', error);
      set({ isLoading: false, error: String(error) });
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `打开项目失败: ${error instanceof Error ? error.message : String(error)}`);
      } catch { /* toast not available */ }
    }
  },

  closeProject: () => {
    // 关闭前保存缓存
    get().saveCurrentCache();
    set({ currentProject: null, error: null });
  },

  /** 保存当前项目的缓存数据 */
  saveCurrentCache: async () => {
    const { currentProject } = get();
    if (!currentProject?.path) return;

    try {
      const fileState = useFileStore.getState();
      const chatState = useChatStore.getState();

      await saveProjectCache(currentProject.path, {
        tabs: fileState.tabs.map((t) => ({ path: t.path })),
        activeTabPath: (() => {
          const active = fileState.tabs.find((t) => t.id === fileState.activeTabId);
          return active?.path ?? null;
        })(),
        chatMessages: chatState.messages,
      });
    } catch (err) {
      console.warn('[ProjectStore] Failed to save cache:', err);
    }
  },

  /** 恢复当前项目的缓存数据 */
  restoreCurrentCache: async () => {
    const { currentProject } = get();
    if (!currentProject?.path) return;

    // 先清空旧项目状态（标签页 + 聊天），再尝试恢复新项目的缓存
    useFileStore.getState().clearTabs();
    useChatStore.getState().resetMessages();

    try {
      const cache = await loadProjectCache(currentProject.path);
      if (!cache) return;

      // 恢复标签页
      const tabPaths = cache.tabs.map((t) => t.path);
      if (tabPaths.length > 0) {
        await useFileStore.getState().restoreTabs(tabPaths, cache.activeTabPath);
      }

      // 恢复聊天消息
      if (cache.chatMessages && cache.chatMessages.length > 0) {
        useChatStore.getState().restoreMessages(cache.chatMessages);
      }
    } catch (err) {
      console.error('[ProjectStore] Failed to restore cache:', err);
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `恢复项目缓存失败: ${err instanceof Error ? err.message : String(err)}`);
      } catch { /* toast not available */ }
    }
  },
}));
