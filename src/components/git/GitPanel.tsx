/**
 * GitPanel - Git 面板组件 (Codex-inspired)
 *
 * 功能：
 * - 查看变更文件列表
 * - 创建分支（使用用户输入的名字）
 * - 提交变更
 * - 回退 AI 修改
 */

import { useState, useEffect, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { GitBranch, GitCommit, Plus, RotateCcw, RefreshCw, FileCode, Loader2 } from 'lucide-react';
import { ask } from '@tauri-apps/plugin-dialog';
import { safeInvoke } from '../../lib/invoke';
import { useProjectStore } from '../../stores';
import { showToast } from '../ui/Toast';
import { InputDialog } from '../ui/InputDialog';

interface FileStatus {
  path: string;
  status: 'modified' | 'added' | 'deleted' | 'untracked' | 'unknown';
}

const STATUS_COLORS: Record<string, string> = {
  modified: 'text-warning',
  added: 'text-success',
  deleted: 'text-error',
  untracked: 'text-text-tertiary',
  unknown: 'text-text-disabled',
};

const STATUS_LABELS: Record<string, string> = {
  modified: 'M',
  added: 'A',
  deleted: 'D',
  untracked: 'U',
  unknown: '?',
};

function GitPanel() {
  const { t } = useTranslation();
  const [files, setFiles] = useState<FileStatus[]>([]);
  const [branch, setBranch] = useState('');
  const [commitMessage, setCommitMessage] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [isCommitting, setIsCommitting] = useState(false);
  const [branchDialogOpen, setBranchDialogOpen] = useState(false);
  const { currentProject } = useProjectStore();

  const loadStatus = useCallback(async () => {
    if (!currentProject) return;
    setIsLoading(true);
    try {
      const status = await safeInvoke<FileStatus[]>('get_status', { projectPath: currentProject.path });
      setFiles(status || []);
    } catch (error) {
      console.error('Failed to load git status:', error);
      setFiles([]);
    } finally {
      setIsLoading(false);
    }
  }, [currentProject]);

  const loadBranch = useCallback(async () => {
    if (!currentProject) return;
    try {
      const name = await safeInvoke<string>('get_current_branch', { projectPath: currentProject.path });
      setBranch(name || '');
    } catch (error) {
      console.error('Failed to load current branch:', error);
      setBranch('');
    }
  }, [currentProject]);

  useEffect(() => {
    loadStatus();
    loadBranch();
  }, [currentProject, loadStatus, loadBranch]);

  const handleCreateBranchClick = () => {
    setBranchDialogOpen(true);
  };

  const handleBranchDialogConfirm = async (name: string) => {
    setBranchDialogOpen(false);
    if (!name || !currentProject) return;
    try {
      const branchName = await safeInvoke<string>('create_branch', {
        projectPath: currentProject.path,
        name,
      });
      setBranch(branchName || name);
      showToast('success', t('git.branchCreated', { name: branchName }));
      await loadStatus();
    } catch (error) {
      showToast('error', t('git.branchCreateFailed', { error: String(error) }));
    }
  };

  const handleCommit = async () => {
    if (!commitMessage.trim() || !currentProject) return;
    setIsCommitting(true);
    try {
      await safeInvoke('commit_ai_changes', {
        projectPath: currentProject.path,
        message: commitMessage,
      });
      setCommitMessage('');
      await loadStatus();
      await loadBranch();
      showToast('success', t('git.commitSuccess'));
    } catch (error) {
      showToast('error', t('git.commitFailed', { error: String(error) }));
    } finally {
      setIsCommitting(false);
    }
  };

  const handleRevert = async () => {
    if (!currentProject) return;
    const confirmed = await ask(t('git.revertConfirm'), {
      title: t('git.revert'),
      kind: 'warning',
      okLabel: t('git.revert'),
      cancelLabel: t('common.cancel', 'Cancel'),
    });
    if (!confirmed) return;
    try {
      await safeInvoke('revert_ai_changes', { projectPath: currentProject.path });
      await loadStatus();
      await loadBranch();
      showToast('success', t('git.revertSuccess'));
    } catch (error) {
      showToast('error', t('git.revertFailed', { error: String(error) }));
    }
  };

  return (
    <div className="h-full flex flex-col bg-surface-base">
      {/* Header */}
      <div className="px-3 py-2 border-b border-border-subtle flex items-center justify-between">
        <div className="flex items-center gap-2">
          <GitBranch size={14} className="text-text-tertiary" />
          <span className="text-[11px] font-semibold uppercase tracking-wider text-text-tertiary">{t('git.title')}</span>
          {branch && (
            <span className="px-1.5 py-0.5 text-[10px] font-medium bg-surface-active text-text-secondary rounded-sm border border-border-subtle">
              {branch}
            </span>
          )}
        </div>
        <div className="flex items-center gap-0.5">
          <button
            onClick={handleCreateBranchClick}
            className="p-1 rounded-sm text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors"
            title={t('git.newBranch')}
          >
            <Plus size={13} />
          </button>
          <button
            onClick={() => { loadStatus(); loadBranch(); }}
            className="p-1 rounded-sm text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors"
            title={t('common.refresh', 'Refresh')}
          >
            <RefreshCw size={13} className={isLoading ? 'animate-spin' : ''} />
          </button>
        </div>
      </div>

      {/* Changes */}
      <div className="flex-1 overflow-y-auto p-2">
        {!currentProject ? (
          <div className="flex flex-col items-center justify-center h-full text-center">
            <GitBranch size={24} className="text-text-disabled mb-2" />
            <p className="text-[12px] text-text-tertiary">{t('git.noProject')}</p>
          </div>
        ) : isLoading ? (
          <div className="flex items-center justify-center h-full">
            <Loader2 size={16} className="text-text-tertiary animate-spin" />
          </div>
        ) : files.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-center">
            <GitCommit size={24} className="text-text-disabled mb-2" />
            <p className="text-[12px] text-text-tertiary">{t('git.noChanges')}</p>
          </div>
        ) : (
          <div className="space-y-0.5">
            {files.map((file) => (
              <div
                key={file.path}
                className="flex items-center gap-2 px-2 py-1.5 rounded-sm hover:bg-surface-hover transition-colors group"
              >
                <span className={`text-[10px] font-mono font-bold w-4 ${STATUS_COLORS[file.status]}`}>
                  {STATUS_LABELS[file.status]}
                </span>
                <FileCode size={12} className={`shrink-0 ${STATUS_COLORS[file.status]}`} />
                <span className="text-[12px] text-text-secondary truncate">{file.path}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Commit Area */}
      {currentProject && (
        <div className="p-2 border-t border-border-subtle">
          <textarea
            value={commitMessage}
            onChange={(e) => setCommitMessage(e.target.value)}
            placeholder={t('git.commitMessage')}
            rows={2}
            className="w-full px-2 py-1.5 text-[12px] bg-surface-overlay border border-border-subtle rounded-md text-text-primary placeholder:text-text-disabled resize-none focus:outline-none"
          />
          <div className="flex gap-1.5 mt-1.5">
            <button
              onClick={handleCommit}
              disabled={!commitMessage.trim() || isCommitting}
              className="flex-1 flex items-center justify-center gap-1 px-3 py-1.5 text-[11px] font-medium bg-success text-white rounded-md hover:bg-green-600 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              {isCommitting ? (
                <Loader2 size={12} className="animate-spin" />
              ) : (
                <GitCommit size={12} />
              )}
              {t('git.commit')}
            </button>
            <button
              onClick={handleRevert}
              className="flex items-center justify-center gap-1 px-2 py-1.5 text-[11px] font-medium bg-surface-overlay border border-border-subtle text-text-tertiary rounded-md hover:border-error hover:text-error transition-all"
            >
              <RotateCcw size={11} />
              {t('git.revert')}
            </button>
          </div>
        </div>
      )}

      {/* Branch Creation Dialog */}
      <InputDialog
        open={branchDialogOpen}
        title={t('git.newBranch')}
        label={t('git.branchNamePrompt')}
        placeholder="feature/..."
        okLabel={t('common.ok', 'OK')}
        cancelLabel={t('common.cancel', 'Cancel')}
        onConfirm={handleBranchDialogConfirm}
        onCancel={() => setBranchDialogOpen(false)}
      />
    </div>
  );
}

const GitPanelMemo = memo(GitPanel);
export { GitPanelMemo as GitPanel };
export default GitPanelMemo;
