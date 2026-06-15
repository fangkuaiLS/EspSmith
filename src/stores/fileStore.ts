/**
 * 文件状态管理
 */

import { create } from 'zustand';
import { safeInvoke } from '../lib/invoke';
import { listen } from '@tauri-apps/api/event';
import type { FileEntry } from '../types';

interface FileTab {
  id: string;
  path: string;
  name: string;
  content: string;
  modified: boolean;
  deleted?: boolean;
}

interface FileState {
  files: FileEntry[];
  tabs: FileTab[];
  activeTabId: string | null;
  isLoading: boolean;
  refreshVersion: number;
  cursorLine: number;
  cursorColumn: number;
  editorLanguage: string;

  // Actions
  loadDirectory: (path: string) => Promise<void>;
  openFile: (path: string) => Promise<void>;
  closeTab: (id: string) => void;
  /** 关闭并重新打开指定路径的文件（用于外部修改后刷新） */
  reloadFileByPath: (filePath: string) => Promise<void>;
  setActiveTab: (id: string) => void;
  updateTabContent: (id: string, content: string) => void;
  saveFile: (id: string, safeMode?: boolean) => Promise<void>;
  refreshOpenTabs: () => Promise<void>;
  updateCursorPosition: (line: number, column: number) => void;
  updateEditorLanguage: (language: string) => void;
  /** 批量恢复标签页（用于项目缓存恢复），返回打开的标签页路径列表 */
  restoreTabs: (paths: string[], activePath?: string | null) => Promise<string[]>;
  /** 清空所有标签页（切换项目时调用） */
  clearTabs: () => void;
}

export const useFileStore = create<FileState>((set, get) => ({
  files: [],
  tabs: [],
  activeTabId: null,
  isLoading: false,
  refreshVersion: 0,
  cursorLine: 1,
  cursorColumn: 1,
  editorLanguage: '',

  loadDirectory: async (path) => {
    set({ isLoading: true });
    try {
      const files = await safeInvoke<FileEntry[]>('list_directory', { path });
      set((state) => ({
        files: files || [],
        isLoading: false,
        refreshVersion: state.refreshVersion + 1,
      }));
    } catch {
      set({ isLoading: false });
    }
  },

  openFile: async (path) => {
    const { tabs } = get();
    const existingTab = tabs.find((t) => t.path === path);
    if (existingTab) {
      set({ activeTabId: existingTab.id });
      return;
    }

    try {
      const content = await safeInvoke<string>('read_file', { path });
      if (content === null) return;
      const name = path.split(/[\\/]/).pop() || 'untitled';
      const newTab: FileTab = {
        id: `tab-${Date.now()}`,
        path,
        name,
        content,
        modified: false,
      };
      set((state) => ({
        tabs: [...state.tabs, newTab],
        activeTabId: newTab.id,
      }));
    } catch (error) {
      console.error('Failed to open file:', error);
    }
  },

  closeTab: (id) => {
    set((state) => {
      const newTabs = state.tabs.filter((t) => t.id !== id);
      let newActiveId = state.activeTabId;
      if (state.activeTabId === id) {
        const idx = state.tabs.findIndex((t) => t.id === id);
        newActiveId = newTabs[idx - 1]?.id || newTabs[0]?.id || null;
      }
      return { tabs: newTabs, activeTabId: newActiveId };
    });
  },

  /** 关闭并重新打开指定路径的文件（用于外部修改后刷新） */
  reloadFileByPath: async (filePath: string) => {
    const { tabs, closeTab } = get();
    // 统一为正斜杠比较，兼容 Windows 反斜杠路径
    const normalizedPath = filePath.replace(/\\/g, '/');
    const tab = tabs.find((t) => t.path.replace(/\\/g, '/') === normalizedPath);
    if (tab) {
      closeTab(tab.id);
    }
    // 短暂延迟确保文件句柄释放后再打开
    await new Promise((r) => setTimeout(r, 100));
    await get().openFile(filePath);
  },

  setActiveTab: (id) => {
    set({ activeTabId: id });
  },

  updateTabContent: (id, content) => {
    set((state) => ({
      tabs: state.tabs.map((t) =>
        t.id === id ? { ...t, content, modified: true } : t
      ),
    }));
  },

  saveFile: async (id, safeMode = false) => {
    const tab = get().tabs.find((t) => t.id === id);
    if (!tab) return;

    try {
      await safeInvoke('write_file', {
        path: tab.path,
        content: tab.content,
        safeMode,
      });
      set((state) => ({
        tabs: state.tabs.map((t) =>
          t.id === id ? { ...t, modified: false } : t
        ),
      }));
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('success', '文件已保存');
      } catch { /* toast not available */ }
    } catch (error) {
      console.error('Failed to save file:', error);
      try {
        const { showToast } = await import('../components/ui/Toast');
        showToast('error', `保存失败: ${error instanceof Error ? error.message : String(error)}`);
      } catch { /* toast not available */ }
    }
  },

  // 刷新所有未修改的标签页内容（AI 修改文件后自动同步）
  refreshOpenTabs: async () => {
    const { tabs } = get();
    for (const tab of tabs) {
      if (tab.modified) continue; // 用户有未保存修改，跳过
      try {
        const content = await safeInvoke<string>('read_file', { path: tab.path });
        if (content !== null) {
          set((state) => ({
            tabs: state.tabs.map((t) =>
              t.id === tab.id ? { ...t, content, modified: false, deleted: false } : t
            ),
          }));
        } else {
          // safeInvoke 返回 null 表示文件不存在或无法读取
          set((state) => ({
            tabs: state.tabs.map((t) =>
              t.id === tab.id ? { ...t, deleted: true } : t
            ),
          }));
        }
      } catch {
        // unexpected error
      }
    }
  },

  // 批量恢复标签页（项目缓存恢复用）
  restoreTabs: async (paths: string[], activePath?: string | null) => {
    const openedPaths: string[] = [];
    // 清空现有标签页
    set({ tabs: [], activeTabId: null });

    for (const path of paths) {
      try {
        const content = await safeInvoke<string>('read_file', { path });
        if (content === null) continue; // 文件不存在，跳过
        const name = path.split(/[\\/]/).pop() || 'untitled';
        const newTab: FileTab = {
          id: `tab-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          path,
          name,
          content,
          modified: false,
        };
        set((state) => ({
          tabs: [...state.tabs, newTab],
        }));
        openedPaths.push(path);
      } catch {
        // 文件不可读，跳过
      }
    }

    // 设置活跃标签页
    if (activePath) {
      const { tabs: currentTabs } = get();
      const targetTab = currentTabs.find((t) => t.path === activePath);
      if (targetTab) {
        set({ activeTabId: targetTab.id });
      } else if (currentTabs.length > 0) {
        set({ activeTabId: currentTabs[0].id });
      }
    } else {
      const { tabs: currentTabs } = get();
      if (currentTabs.length > 0) {
        set({ activeTabId: currentTabs[0].id });
      }
    }

    return openedPaths;
  },

  updateCursorPosition: (line, column) => {
    set({ cursorLine: line, cursorColumn: column });
  },

  updateEditorLanguage: (language) => {
    set({ editorLanguage: language });
  },

  // 清空所有标签页（切换项目时调用）
  clearTabs: () => {
    set({ tabs: [], activeTabId: null });
  },
}));

// ─── AI 文件变更防抖 ────────────────────────────────────────────
// 收集短时间内的所有文件变更，合并后一次性刷新，避免频繁闪烁
const DEBOUNCE_MS = 1000;
const _pendingPaths = new Set<string>();
let _debounceTimer: ReturnType<typeof setTimeout> | null = null;

function _flushPendingChanges() {
  const paths = Array.from(_pendingPaths);
  _pendingPaths.clear();
  _debounceTimer = null;
  if (paths.length === 0) return;

  const { tabs } = useFileStore.getState();
  const tabsToRefresh = tabs.filter(
    (t) => !t.modified && paths.some((p) => t.path === p || t.path.endsWith(p) || p.endsWith(t.path))
  );

  if (tabsToRefresh.length === 0) return;

  Promise.all(
    tabsToRefresh.map((tab) =>
      safeInvoke<string>('read_file', { path: tab.path })
        .then((content) => ({ tabId: tab.id, content }))
        .catch(() => ({ tabId: tab.id, content: null }))
    )
  ).then((results) => {
    useFileStore.setState((state) => ({
      activeTabId: state.activeTabId,
      tabs: state.tabs.map((t) => {
        const r = results.find((x) => x.tabId === t.id);
        if (!r) return t;
        if (r.content !== null && r.content !== t.content) {
          return { ...t, content: r.content, modified: false, deleted: false };
        }
        if (r.content === null) {
          return { ...t, deleted: true };
        }
        return t;
      }),
    }));
  });
}

listen<string>('ai-file-changed', (event) => {
  const changedPath = event.payload;
  if (!changedPath) return;

  _pendingPaths.add(changedPath);

  if (_debounceTimer) {
    clearTimeout(_debounceTimer);
  }
  _debounceTimer = setTimeout(_flushPendingChanges, DEBOUNCE_MS);
}).catch(() => {});

// ─── AGENTS.md 动态刷新 ──────────────────────────────────────────
// 切换 AI 引擎后，后端重写 AGENTS.md 并发送此事件，
// 前端关闭并重新打开该文件以绕过 Windows 文件锁定问题
listen<string>('agents_updated', (event) => {
  const filePath = event.payload;
  if (!filePath) return;
  console.log('[fileStore] agents_updated:', filePath);
  useFileStore.getState().reloadFileByPath(filePath);
}).catch(() => {});

// ─── sdkconfig 动态刷新 ──────────────────────────────────────────
// 保存 SDK 配置后，后端通过原子写入更新 sdkconfig 并发送此事件，
// 前端关闭并重新打开该文件以显示最新内容
listen<string>('sdkconfig_updated', (event) => {
  const filePath = event.payload;
  if (!filePath) return;
  console.log('[fileStore] sdkconfig_updated:', filePath);
  useFileStore.getState().reloadFileByPath(filePath);
}).catch(() => {});

// ─── 窗口标题跟随当前文件 ──────────────────────────────────────
function updateWindowTitle() {
  const { tabs, activeTabId } = useFileStore.getState();
  const tab = tabs.find((t) => t.id === activeTabId);
  if (tab) {
    const title = tab.modified ? `${tab.name} • EspSmith` : `${tab.name} — EspSmith`;
    import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      getCurrentWindow().setTitle(title).catch(() => {});
    });
  } else {
    import('@tauri-apps/api/window').then(({ getCurrentWindow }) => {
      getCurrentWindow().setTitle('EspSmith').catch(() => {});
    });
  }
}

useFileStore.subscribe((state, prevState) => {
  if (state.activeTabId === prevState.activeTabId) {
    // 同一标签页，检查修改状态是否变化
    const prevTab = prevState.tabs.find((t) => t.id === state.activeTabId);
    const currTab = state.tabs.find((t) => t.id === state.activeTabId);
    if (prevTab?.modified === currTab?.modified) return;
  }
  updateWindowTitle();
});
