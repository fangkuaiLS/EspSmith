/**
 * FileTree - 文件树组件 (VS Code 风格)
 *
 * 功能：
 * - 递归显示目录结构
 * - 展开/折叠文件夹
 * - 点击打开文件
 * - VS Code 风格右键菜单（复制路径、重命名、删除、复制、剪切/粘贴）
 * - 内联重命名（F2 或右键菜单）
 * - 删除确认对话框
 */

import { useState, useEffect, useCallback, useRef, memo } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Folder, FolderOpen, File,
  ChevronRight, RefreshCw, FilePlus, FolderPlus, MoreHorizontal,
  Copy, Clipboard, Scissors, ClipboardPaste, Trash2, Pencil,
  ArrowUpRight, Search, Check, X
} from 'lucide-react';
import { safeInvoke } from '../../lib/invoke';
import { getFileIcon as getSharedFileIcon } from '../../lib/fileIcons';
import { InputDialog } from '../ui/InputDialog';
import { showToast } from '../ui/Toast';
import { useFileStore, useProjectStore } from '../../stores';
import type { FileEntry } from '../../types';

// ==================== 常量 ====================

function getFileIcon(name: string) {
  return getSharedFileIcon(name);
}

// 剪贴板状态（应用内，不依赖系统剪贴板）
type ClipboardItem = { path: string; name: string; is_dir: boolean; operation: 'copy' | 'cut' } | null;

// ==================== TreeNode ====================

interface TreeNodeProps {
  entry: FileEntry;
  level: number;
  onOpenFile: (path: string) => void;
  onRefresh: () => void;
  clipboard: ClipboardItem;
  setClipboard: (item: ClipboardItem) => void;
}

function TreeNode({ entry, level, onOpenFile, onRefresh, clipboard, setClipboard }: TreeNodeProps) {
  const { t } = useTranslation();
  const refreshVersion = useFileStore((s) => s.refreshVersion);
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<FileEntry[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);

  // 内联重命名状态
  const [isRenaming, setIsRenaming] = useState(false);
  const [renameValue, setRenameValue] = useState(entry.name);
  const renameInputRef = useRef<HTMLInputElement>(null);

  // 删除确认
  const [confirmDelete, setConfirmDelete] = useState(false);

  // 新建文件/文件夹对话框（在当前目录下新建）
  const [newDialog, setNewDialog] = useState<'newFile' | 'newFolder' | null>(null);

  const loadChildren = useCallback(async () => {
    if (entry.is_dir) {
      setIsLoading(true);
      try {
        const files = await safeInvoke<FileEntry[]>('list_directory', { path: entry.path });
        setChildren(files || []);
      } catch (error) {
        console.error('Failed to load directory:', error);
      } finally {
        setIsLoading(false);
      }
    }
  }, [entry.path, entry.is_dir]);

  useEffect(() => {
    if (expanded && children.length === 0 && !isLoading) {
      loadChildren();
    }
  }, [expanded]);

  useEffect(() => {
    if (expanded && entry.is_dir) {
      loadChildren();
    }
  }, [refreshVersion]);

  // F2 快捷键触发重命名（仅当此节点有焦点时）
  useEffect(() => {
    if (!contextMenu && !isRenaming) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'F2') {
        e.preventDefault();
        setIsRenaming(true);
        setRenameValue(entry.name);
        setTimeout(() => renameInputRef.current?.focus(), 50);
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [contextMenu, isRenaming, entry.name]);

  const handleClick = () => {
    if (isRenaming) return;
    if (entry.is_dir) {
      setExpanded(!expanded);
    } else {
      onOpenFile(entry.path);
    }
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY });
  };

  const closeContextMenu = () => setContextMenu(null);

  // ===== 操作 =====

  const execRename = async (newName: string) => {
    if (!newName.trim() || newName === entry.name) {
      setIsRenaming(false);
      return;
    }
    try {
      await safeInvoke('rename_file', { oldPath: entry.path, newName: newName.trim() });
      setIsRenaming(false);
      onRefresh();
      showToast('success', t('toast.renamed', { name: newName }));
    } catch (err) {
      showToast('error', t('toast.renameFailed', { error: String(err) }));
      setIsRenaming(false);
    }
  };

  const handleRenameStart = () => {
    closeContextMenu();
    setIsRenaming(true);
    setRenameValue(entry.name);
    setTimeout(() => renameInputRef.current?.focus(), 50);
  };

  const handleRenameCancel = () => {
    setRenameValue(entry.name);
    setIsRenaming(false);
  };

  const handleDelete = async () => {
    closeContextMenu();
    setConfirmDelete(true);
  };

  const confirmDeleteAction = async () => {
    setConfirmDelete(false);
    try {
      await safeInvoke('delete_file', { path: entry.path });
      onRefresh();
      showToast('success', t('toast.deleted', { name: entry.name }));
    } catch (err) {
      showToast('error', t('toast.deleteFailed', { error: String(err) }));
    }
  };

  const handleCopy = () => {
    closeContextMenu();
    setClipboard({ path: entry.path, name: entry.name, is_dir: entry.is_dir, operation: 'copy' });
    showToast('info', t('toast.copied', { name: entry.name }));
  };

  const handleCut = () => {
    closeContextMenu();
    setClipboard({ path: entry.path, name: entry.name, is_dir: entry.is_dir, operation: 'cut' });
    showToast('info', t('toast.cut', { name: entry.name }));
  };

  const handlePaste = async () => {
    closeContextMenu();
    if (!clipboard || !entry.is_dir) return;
    try {
      // 对于粘贴，简化为复制文件到当前目录
      // 这里用 duplicate 作为简化实现，实际应该读原文件内容再写
      const newName = clipboard.operation === 'copy'
        ? clipboard.name
        : clipboard.name; // cut 模式下也是复制内容（因为跨进程 fs 无法 move）

      if (clipboard.operation === 'cut') {
        // Move: rename 到新位置
        await safeInvoke('rename_file', { oldPath: clipboard.path, newName: `${entry.path}\\${newName}` });
        setClipboard(null);
      } else {
        // Copy: 读取源文件内容，写入目标
        const content = await safeInvoke<string>('read_file', { path: clipboard.path });
        await safeInvoke('create_file', { parentPath: entry.path, name: newName, content: content || '' });
      }
      onRefresh();
      showToast('success', t('toast.pasted', { name: newName }));
    } catch (err) {
      showToast('error', t('toast.pasteFailed', { error: String(err) }));
    }
  };

  const handleDuplicate = async () => {
    closeContextMenu();
    try {
      const result = await safeInvoke<FileEntry>('duplicate_file', { path: entry.path });
      if (result) {
        onRefresh();
        showToast('success', t('toast.duplicated', { name: result.name }));
      }
    } catch (err) {
      showToast('error', t('toast.duplicateFailed', { error: String(err) }));
    }
  };

  const handleCopyPath = () => {
    closeContextMenu();
    navigator.clipboard.writeText(entry.path).then(() => {
      showToast('success', t('toast.fullPathCopied'));
    }).catch(() => {
      showToast('error', t('toast.copyPathFailed'));
    });
  };

  const handleCopyRelativePath = () => {
    closeContextMenu();
    // 从 projectStore 获取真实的项目根目录，而非从路径推断
    const projectRoot = useProjectStore.getState().currentProject?.path || '';
    const relative = projectRoot
      ? entry.path.replace(projectRoot, '').replace(/^[\\/]/, '')
      : entry.name;
    navigator.clipboard.writeText(relative || entry.name).then(() => {
      showToast('success', t('toast.relativePathCopied'));
    }).catch(() => {
      showToast('error', t('toast.copyPathFailed'));
    });
  };

  const handleNewFileInDir = () => {
    closeContextMenu();
    setExpanded(true);
    setNewDialog('newFile');
  };

  const handleNewFolderInDir = () => {
    closeContextMenu();
    setExpanded(true);
    setNewDialog('newFolder');
  };

  const handleNewDialogConfirm = async (value: string) => {
    const type = newDialog;
    setNewDialog(null);
    if (!value.trim()) return;

    try {
      if (type === 'newFile') {
        await safeInvoke('create_file', { parentPath: entry.path, name: value, content: '' });
        showToast('success', t('toast.fileCreated', { name: value }));
      } else if (type === 'newFolder') {
        await safeInvoke('create_folder', { parentPath: entry.path, name: value });
        showToast('success', t('toast.folderCreated', { name: value }));
      }
      // 刷新子目录
      if (expanded) {
        const files = await safeInvoke<FileEntry[]>('list_directory', { path: entry.path });
        setChildren(files || []);
      }
      onRefresh();
    } catch (err) {
      showToast('error', type === 'newFile' ? t('toast.fileCreateFailed', { error: String(err) }) : t('toast.folderCreateFailed', { error: String(err) }));
    }
  };

  // ===== 渲染 =====

  const IconComponent = entry.is_dir
    ? (expanded ? FolderOpen : Folder)
    : getFileIcon(entry.name);

  const isCut = clipboard?.path === entry.path && clipboard?.operation === 'cut';

  return (
    <div onClick={closeContextMenu}>
      <div
        className={`group flex items-center gap-1.5 px-2 py-1 cursor-pointer text-[13px] transition-colors ${
          isCut ? 'opacity-50' : ''
        }`}
        style={{ paddingLeft: `${level * 14 + 8}px` }}
        onClick={handleClick}
        onContextMenu={handleContextMenu}
      >
        {entry.is_dir && (
          <ChevronRight
            size={14}
            className={`text-text-tertiary shrink-0 transition-transform duration-150 ${
              expanded ? 'rotate-90' : ''
            }`}
          />
        )}
        {!entry.is_dir && <div className="w-[14px] shrink-0" />}

        <IconComponent
          size={15}
          className={`shrink-0 ${entry.is_dir ? expanded ? 'text-accent' : 'text-text-tertiary' : 'text-text-tertiary'}`}
        />

        {/* 名称 / 内联重命名输入框 */}
        {isRenaming ? (
          <div className="flex-1 flex items-center gap-1 min-w-0" onClick={(e) => e.stopPropagation()}>
            <input
              ref={renameInputRef}
              type="text"
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') execRename(renameValue);
                if (e.key === 'Escape') handleRenameCancel();
              }}
              onBlur={() => execRename(renameValue)}
              className="flex-1 px-1.5 py-0.5 text-[12px] bg-surface-overlay border border-accent rounded-sm text-text-primary outline-none min-w-[60px]"
            />
            <button
              onClick={() => execRename(renameValue)}
              className="p-0.5 rounded-sm text-green-500 hover:bg-surface-hover"
              title={t('common.confirm')}
            >
              <Check size={12} />
            </button>
            <button
              onClick={handleRenameCancel}
              className="p-0.5 rounded-sm text-text-tertiary hover:bg-surface-hover"
              title={t('common.cancel')}
            >
              <X size={12} />
            </button>
          </div>
        ) : (
          <span className="truncate text-text-secondary group-hover:text-text-primary transition-colors flex-1 min-w-0">
            {entry.name}
          </span>
        )}

        {!isRenaming && (
          <button
            className="ml-auto p-0.5 rounded-sm opacity-0 group-hover:opacity-100 hover:bg-surface-hover text-text-tertiary transition-all shrink-0"
            onClick={(e) => { e.stopPropagation(); setContextMenu({ x: e.clientX, y: e.clientY }); }}
          >
            <MoreHorizontal size={12} />
          </button>
        )}
      </div>

      {/* Children */}
      {expanded && entry.is_dir && (
        <div>
          {isLoading ? (
            <div className="text-[12px] text-text-tertiary pl-8 py-1">{t('leftPanel.loading')}</div>
          ) : (
            children.map((child) => (
              <TreeNode
                key={child.path}
                entry={child}
                level={level + 1}
                onOpenFile={onOpenFile}
                onRefresh={onRefresh}
                clipboard={clipboard}
                setClipboard={setClipboard}
              />
            ))
          )}
        </div>
      )}

      {/* 删除确认对话框 */}
      {confirmDelete && (
        <>
          <div className="fixed inset-0 z-50" onClick={() => setConfirmDelete(false)} />
          <div className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-[60] bg-surface-elevated border border-border-default rounded-xl shadow-2xl p-5 w-[380px] animate-scale-in">
            <h3 className="text-[14px] font-semibold text-text-primary mb-2">
              {t('fileTree.deleteConfirm')}
            </h3>
            <p className="text-[12px] text-text-secondary mb-1">
              {t('fileTree.deleteWarning', { name: entry.name })}
            </p>
            {entry.is_dir && (
              <p className="text-[11px] text-error mb-4">
                {t('fileTree.deleteDirWarning')}
              </p>
            )}
            <div className="flex justify-end gap-2 mt-4">
              <button
                onClick={() => setConfirmDelete(false)}
                className="px-4 py-2 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary transition-all"
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={confirmDeleteAction}
                className="px-4 py-2 text-[12px] font-medium bg-error text-white rounded-lg hover:bg-red-600 transition-all"
              >
                {t('common.delete')}
              </button>
            </div>
          </div>
        </>
      )}

      {/* Context Menu */}
      {contextMenu && (
        <>
          <div className="fixed inset-0 z-40" onClick={closeContextMenu} />
          <div
            className="fixed z-50 bg-surface-overlay border border-border-default rounded-xl shadow-xl py-1.5 min-w-[220px] animate-scale-in"
            style={{ left: Math.min(contextMenu.x, window.innerWidth - 230), top: Math.min(contextMenu.y, window.innerHeight - 420) }}
          >
            {entry.is_dir ? (
              <>
                <MenuItem icon={FilePlus} label={t('leftPanel.newFile')} shortcut="Ctrl+N" onClick={handleNewFileInDir} />
                <MenuItem icon={FolderPlus} label={t('leftPanel.newFolder')} shortcut="" onClick={handleNewFolderInDir} />
                <Separator />
                <MenuItem icon={FolderOpen} label={t('leftPanel.expandAll')} onClick={() => setExpanded(true)} />
                <MenuItem icon={ChevronRight} label={t('leftPanel.collapseAll')} onClick={() => setExpanded(false)} />
                <Separator />
                <MenuItem icon={ClipboardPaste} label={t('fileTree.paste')} shortcut="Ctrl+V" onClick={handlePaste} disabled={!clipboard} />
                <Separator />
                <MenuItem icon={Copy} label={t('fileTree.copyPath')} shortcut="Ctrl+Shift+C" onClick={handleCopyPath} />
                <MenuItem icon={Copy} label={t('fileTree.copyRelativePath')} shortcut="Ctrl+K Ctrl+C" onClick={handleCopyRelativePath} />
                <Separator />
                <MenuItem icon={Pencil} label={t('fileTree.rename')} shortcut="F2" onClick={handleRenameStart} />
                <MenuItem icon={Trash2} label={t('fileTree.delete')} shortcut="Del" onClick={handleDelete} danger />
              </>
            ) : (
              <>
                <MenuItem icon={File} label={t('fileTree.openInEditor')} onClick={() => { onOpenFile(entry.path); closeContextMenu(); }} />
                <MenuItem icon={ArrowUpRight} label={t('fileTree.openToSide')} disabled />
                <Separator />
                <MenuItem icon={Scissors} label={t('fileTree.cut')} shortcut="Ctrl+X" onClick={handleCut} />
                <MenuItem icon={Clipboard} label={t('fileTree.copy')} shortcut="Ctrl+C" onClick={handleCopy} />
                <MenuItem icon={ClipboardPaste} label={t('fileTree.paste')} shortcut="Ctrl+V" onClick={handlePaste} disabled={!clipboard || !entry.is_dir} />
                <Separator />
                <MenuItem icon={Copy} label={t('fileTree.copyPath')} shortcut="Ctrl+Shift+C" onClick={handleCopyPath} />
                <MenuItem icon={Copy} label={t('fileTree.copyRelativePath')} shortcut="Ctrl+K Ctrl+C" onClick={handleCopyRelativePath} />
                <Separator />
                <MenuItem icon={Copy} label={t('fileTree.duplicate')} shortcut="" onClick={handleDuplicate} />
                <Separator />
                <MenuItem icon={Pencil} label={t('fileTree.rename')} shortcut="F2" onClick={handleRenameStart} />
                <MenuItem icon={Search} label={t('fileTree.findInFolder')} shortcut="Ctrl+Shift+F" onClick={() => window.dispatchEvent(new CustomEvent('open-global-search'))} />
                <Separator />
                <MenuItem icon={Trash2} label={t('fileTree.delete')} shortcut="Del" onClick={handleDelete} danger />
              </>
            )}
          </div>
        </>
      )}

      {/* 新建文件/文件夹对话框 */}
      <InputDialog
        open={newDialog !== null}
        title={newDialog === 'newFile' ? t('dialog.newFile') : t('dialog.newFolder')}
        placeholder={newDialog === 'newFile' ? 'untitled.c' : 'components'}
        label={newDialog === 'newFile' ? t('dialog.fileName') : t('dialog.folderName')}
        onConfirm={handleNewDialogConfirm}
        onCancel={() => setNewDialog(null)}
      />
    </div>
  );
}

// ==================== 菜单项子组件 ====================

function Separator() {
  return <div className="border-t border-border-subtle my-1" />;
}

function MenuItem({
  icon: Icon,
  label,
  shortcut,
  danger,
  disabled,
  onClick,
}: {
  icon: React.ComponentType<{ size?: number | string; className?: string }>;
  label: string;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <div
      onClick={disabled ? undefined : onClick}
      className={`flex items-center gap-2.5 px-3 py-1.5 text-[12px] transition-colors ${
        disabled
          ? 'text-text-disabled cursor-default'
          : danger
          ? 'text-error hover:bg-error-muted cursor-pointer'
          : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary cursor-pointer'
      }`}
    >
      <Icon size={14} className="shrink-0" />
      <span className="flex-1">{label}</span>
      {shortcut && (
        <span className="text-[11px] text-text-disabled shrink-0 ml-4">{shortcut}</span>
      )}
    </div>
  );
}

// ==================== 文件树主组件 ====================

function FileTree() {
  const { t } = useTranslation();
  const { currentProject } = useProjectStore();
  const { files, loadDirectory, openFile } = useFileStore();

  // 内联输入对话框（新建文件/文件夹）
  type DialogType = 'newFile' | 'newFolder' | null;
  const [dialogType, setDialogType] = useState<DialogType>(null);

  // 剪贴板
  const [clipboard, setClipboard] = useState<ClipboardItem>(null);

  useEffect(() => {
    if (currentProject) {
      loadDirectory(currentProject.path);
    }
  }, [currentProject, loadDirectory]);

  const handleOpenFile = (path: string) => {
    openFile(path);
  };

  const handleNewFile = () => setDialogType('newFile');
  const handleNewFolder = () => setDialogType('newFolder');

  const handleDialogConfirm = async (value: string) => {
    setDialogType(null);
    if (!currentProject) return;

    if (dialogType === 'newFile') {
      try {
        await safeInvoke('create_file', { parentPath: currentProject.path, name: value, content: '' });
        loadDirectory(currentProject.path);
        showToast('success', t('toast.fileCreated', { name: value }));
      } catch (err) {
        showToast('error', t('toast.fileCreateFailed', { error: String(err) }));
      }
    } else if (dialogType === 'newFolder') {
      try {
        await safeInvoke('create_folder', { parentPath: currentProject.path, name: value });
        loadDirectory(currentProject.path);
        showToast('success', t('toast.folderCreated', { name: value }));
      } catch (err) {
        showToast('error', t('toast.folderCreateFailed', { error: String(err) }));
      }
    }
  };

  const handleDialogCancel = () => setDialogType(null);
  const handleRefresh = () => currentProject && loadDirectory(currentProject.path);

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-2 border-b border-border-subtle">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-text-tertiary">
          {t('leftPanel.explorer')}
        </span>
        <div className="flex items-center gap-0.5">
          <button className="p-1 rounded-sm text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors" title={t('leftPanel.newFile')} onClick={handleNewFile}>
            <FilePlus size={14} />
          </button>
          <button className="p-1 rounded-sm text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors" title={t('leftPanel.newFolder')} onClick={handleNewFolder}>
            <FolderPlus size={14} />
          </button>
          <button className="p-1 rounded-sm text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors" title={t('leftPanel.refresh')} onClick={handleRefresh}>
            <RefreshCw size={14} />
          </button>
        </div>
      </div>

      {/* File list */}
      <div className="flex-1 overflow-auto py-1">
        {!currentProject ? (
          <div className="flex flex-col items-center justify-center h-full text-center px-4">
            <Folder size={32} className="text-text-disabled mb-3" />
            <p className="text-[13px] text-text-tertiary mb-1">{t('leftPanel.noProjectOpen')}</p>
            <p className="text-[11px] text-text-disabled">{t('leftPanel.noProjectHint')}</p>
          </div>
        ) : files.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-center px-4">
            <File size={32} className="text-text-disabled mb-3" />
            <p className="text-[13px] text-text-tertiary">{t('leftPanel.emptyProject')}</p>
            <p className="text-[11px] text-text-disabled">{t('leftPanel.emptyProjectHint')}</p>
          </div>
        ) : (
          files.map((entry) => (
            <TreeNode
              key={entry.path}
              entry={entry}
              level={0}
              onOpenFile={handleOpenFile}
              onRefresh={handleRefresh}
              clipboard={clipboard}
              setClipboard={setClipboard}
            />
          ))
        )}
      </div>

      {/* 内联输入对话框 */}
      <InputDialog
        open={dialogType !== null}
        title={dialogType === 'newFile' ? t('dialog.newFile') : t('dialog.newFolder')}
        placeholder={dialogType === 'newFile' ? 'untitled.c' : 'components'}
        label={dialogType === 'newFile' ? t('dialog.fileName') : t('dialog.folderName')}
        cancelLabel={t('dialog.cancel')}
        okLabel={t('dialog.ok')}
        onConfirm={handleDialogConfirm}
        onCancel={handleDialogCancel}
      />
    </div>
  );
}

const FileTreeMemo = memo(FileTree);
export { FileTreeMemo as FileTree };
export default FileTreeMemo;
