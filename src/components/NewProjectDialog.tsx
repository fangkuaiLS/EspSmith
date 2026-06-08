/**
 * NewProjectDialog — 新建项目（硬件配置表）
 *
 * 不使用模板，直接配置芯片和硬件参数创建项目
 * 全新设计主题：玻璃态卡片 + 渐变背景 + 动态光效
 */

import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { X, FolderOpen, Loader2, Cpu, HardDrive, Sparkles, Layers, AlertTriangle } from 'lucide-react';
import { useSettingsStore } from '../stores';
import { safeInvoke, isTauri } from '../lib/invoke';
import { ChipTargetInfo } from '../types';
import { showToast } from './ui/Toast';

interface NewProjectDialogProps {
  onClose: () => void;
  onProjectCreated: (projectPath: string, chip?: string) => void;
  onOpenSettings?: () => void;
}

type FlashSize = '2MB' | '4MB' | '8MB' | '16MB';

export function NewProjectDialog({ onClose, onProjectCreated, onOpenSettings }: NewProjectDialogProps) {
  const { t } = useTranslation();
  const { idfStatus, settings } = useSettingsStore();

  const [projectName, setProjectName] = useState('my_project');
  const [projectPath, setProjectPath] = useState(settings.defaultProjectPath || '');
  const [selectedChip, setSelectedChip] = useState('ESP32');
  const [flashSize, setFlashSize] = useState<FlashSize>('4MB');
  const [chips, setChips] = useState<ChipTargetInfo[]>([]);
  const [creating, setCreating] = useState(false);

  const idfPath = idfStatus.active?.idf_path || settings.idfPath || '';
  const fullProjectPath = projectPath ? `${projectPath.replace(/[\\/]$/, '')}\\${projectName}` : '';

  useEffect(() => {
    (async () => {
      if (!idfPath) return;
      try {
        const c = await safeInvoke<ChipTargetInfo[]>('idf_get_supported_targets', { idfPath });
        if (c) setChips(c);
      } catch { /* use defaults */ }
    })();
  }, [idfPath]);

  const handleBrowse = useCallback(async () => {
    if (isTauri()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ directory: true, multiple: false, title: t('newProjectDialog.browseTitle') });
        if (selected) {
          setProjectPath(Array.isArray(selected) ? selected[0] : selected);
        }
      } catch { /* ignore */ }
    }
  }, [t]);

  const handleCreate = useCallback(async () => {
    if (!projectName.trim()) {
      showToast('error', t('newProjectDialog.toast.enterName'));
      return;
    }
    if (!projectPath.trim()) {
      showToast('error', t('newProjectDialog.toast.selectPath'));
      return;
    }
    if (!idfPath) {
      showToast('error', t('newProjectDialog.toast.idfNotConfigured'));
      return;
    }

    setCreating(true);
    try {
      const result = await safeInvoke<string>('create_project', {
        config: {
          name: projectName.trim(),
          path: projectPath.trim(),
          chip: selectedChip,
          idf_path: idfPath,
          flash_size: flashSize,
        },
      });
      if (result) {
        showToast('success', t('newProjectDialog.toast.createSuccess', { name: projectName }));
        onProjectCreated(result, selectedChip);
        onClose();
      }
    } catch (err) {
      showToast('error', t('newProjectDialog.toast.createFailed', { error: String(err) }));
    } finally {
      setCreating(false);
    }
  }, [projectName, projectPath, selectedChip, flashSize, idfPath, t, onClose, onProjectCreated]);

  const chipOptions = chips.length > 0 ? chips : [
    { target: 'ESP32', label: 'ESP32', is_preview: false, description: '' },
    { target: 'ESP32-S2', label: 'ESP32-S2', is_preview: false, description: '' },
    { target: 'ESP32-S3', label: 'ESP32-S3', is_preview: false, description: '' },
    { target: 'ESP32-C3', label: 'ESP32-C3', is_preview: false, description: '' },
    { target: 'ESP32-C2', label: 'ESP32-C2', is_preview: false, description: '' },
    { target: 'ESP32-C6', label: 'ESP32-C6', is_preview: false, description: '' },
    { target: 'ESP32-H2', label: 'ESP32-H2', is_preview: false, description: '' },
    { target: 'ESP32-C5', label: 'ESP32-C5', is_preview: false, description: '' },
    { target: 'ESP32-S31', label: 'ESP32-S31', is_preview: false, description: '' },
    { target: 'ESP32-P4', label: 'ESP32-P4', is_preview: false, description: '' },
  ];

  const flashOptions: FlashSize[] = ['2MB', '4MB', '8MB', '16MB'];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-lg animate-fade-in">
      {/* 背景光效 */}
      <div className="absolute inset-0 overflow-hidden pointer-events-none">
        <div className="absolute top-1/3 left-1/3 w-[500px] h-[500px] bg-blue-500/8 rounded-full blur-[120px] animate-pulse-slow" />
        <div className="absolute bottom-1/3 right-1/3 w-[400px] h-[400px] bg-purple-500/8 rounded-full blur-[120px] animate-pulse-slow" style={{ animationDelay: '2s' }} />
      </div>

      <div className="relative bg-surface-elevated/95 backdrop-blur-xl border border-white/10 rounded-2xl shadow-2xl shadow-black/40 w-[540px] max-h-[85vh] overflow-hidden animate-scale-in">
        {/* 顶部渐变条 */}
        <div className="absolute top-0 inset-x-0 h-0.5 bg-gradient-to-r from-blue-500 via-purple-500 to-emerald-500" />

        {/* Header */}
        <div className="flex items-center justify-between px-7 py-5 border-b border-white/5">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-xl bg-gradient-to-br from-blue-500/20 to-purple-500/20 flex items-center justify-center">
              <Sparkles size={16} className="text-blue-400" />
            </div>
            <div>
              <h2 className="text-[16px] font-bold text-text-primary">
                {t('newProjectDialog.title')}
              </h2>
              <p className="text-[11px] text-text-tertiary mt-0.5">
                {t('newProjectDialog.subtitle')}
              </p>
            </div>
          </div>
          <button onClick={onClose} className="p-2 rounded-lg text-text-tertiary hover:text-text-primary hover:bg-white/5 transition-all duration-200">
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="px-7 py-5 space-y-5">
          {/* Project Name */}
          <div>
            <label className="flex items-center gap-1.5 text-[11px] font-semibold text-text-secondary mb-2 uppercase tracking-wider">
              <Layers size={14} />
              {t('newProjectDialog.projectName')}
            </label>
            <input
              type="text"
              value={projectName}
              onChange={(e) => setProjectName(e.target.value)}
              className="w-full px-4 py-3 text-[13px] bg-white/5 border border-white/10 rounded-xl text-text-primary placeholder:text-text-disabled/50 focus:outline-none focus:border-blue-500/50 focus:bg-white/[0.07] focus:ring-4 focus:ring-blue-500/10 transition-all duration-300 font-mono"
              placeholder={t('newProjectDialog.projectNamePlaceholder')}
            />
            {fullProjectPath && (
              <div className="mt-2 flex items-center gap-1.5 text-[11px] text-text-tertiary">
                <FolderOpen size={11} />
                <span className="truncate">{t('newProjectDialog.willCreateAt')} {fullProjectPath}</span>
              </div>
            )}
          </div>

          {/* Project Path */}
          <div>
            <label className="flex items-center gap-1.5 text-[11px] font-semibold text-text-secondary mb-2 uppercase tracking-wider">
              <FolderOpen size={14} />
              {t('newProjectDialog.parentDir')}
            </label>
            <div className="flex gap-2">
              <input
                type="text"
                value={projectPath}
                onChange={(e) => setProjectPath(e.target.value)}
                className="flex-1 px-4 py-3 text-[13px] bg-white/5 border border-white/10 rounded-xl text-text-primary placeholder:text-text-disabled/50 focus:outline-none focus:border-blue-500/50 focus:bg-white/[0.07] focus:ring-4 focus:ring-blue-500/10 transition-all duration-300 font-mono"
                placeholder={t('newProjectDialog.parentDirPlaceholder')}
              />
              <button
                onClick={handleBrowse}
                className="px-4 py-3 bg-white/5 border border-white/10 rounded-xl hover:bg-white/10 hover:border-white/20 text-text-secondary hover:text-text-primary transition-all duration-200"
              >
                <FolderOpen size={16} />
              </button>
            </div>
          </div>

          {/* Hardware Config Table */}
          <div className="border border-white/10 rounded-xl overflow-hidden">
            <div className="bg-white/[0.03] px-4 py-3 border-b border-white/5">
              <span className="text-[11px] font-semibold text-text-secondary uppercase tracking-wider">
                {t('newProjectDialog.hardwareConfig')}
              </span>
            </div>

            <div className="divide-y divide-white/5">
              {/* Chip */}
              <div className="flex items-center px-4 py-3.5 hover:bg-white/[0.02] transition-colors">
                <div className="flex items-center gap-2.5 w-[140px] shrink-0">
                  <div className="p-1.5 rounded-lg bg-blue-500/10">
                    <Cpu size={14} className="text-blue-400" />
                  </div>
                  <span className="text-[12px] font-medium text-text-secondary">{t('newProjectDialog.chipModel')}</span>
                </div>
                <select
                  value={selectedChip}
                  onChange={(e) => setSelectedChip(e.target.value)}
                  className="flex-1 px-3 py-2 text-[13px] bg-white/5 border border-white/10 rounded-lg text-text-primary focus:outline-none focus:border-blue-500/50 focus:ring-4 focus:ring-blue-500/10 transition-all duration-300 cursor-pointer"
                >
                  {chipOptions.map((c) => (
                    <option key={c.target} value={c.target}>
                      {c.label}{c.is_preview ? ` (${t('newProjectDialog.preview')})` : ''}
                    </option>
                  ))}
                </select>
              </div>

              {/* Flash Size */}
              <div className="flex items-center px-4 py-3.5 hover:bg-white/[0.02] transition-colors">
                <div className="flex items-center gap-2.5 w-[140px] shrink-0">
                  <div className="p-1.5 rounded-lg bg-purple-500/10">
                    <HardDrive size={14} className="text-purple-400" />
                  </div>
                  <span className="text-[12px] font-medium text-text-secondary">{t('newProjectDialog.flashSize')}</span>
                </div>
                <select
                  value={flashSize}
                  onChange={(e) => setFlashSize(e.target.value as FlashSize)}
                  className="flex-1 px-3 py-2 text-[13px] bg-white/5 border border-white/10 rounded-lg text-text-primary focus:outline-none focus:border-blue-500/50 focus:ring-4 focus:ring-blue-500/10 transition-all duration-300 cursor-pointer"
                >
                  {flashOptions.map((fs) => (
                    <option key={fs} value={fs}>{fs}</option>
                  ))}
                </select>
              </div>
            </div>
          </div>
        </div>

        {/* Footer */}
        <div className="px-7 py-4 border-t border-white/5 bg-white/[0.02]">
          {!idfPath && (
            <div className="flex items-center gap-2 mb-3 px-1">
              <AlertTriangle size={14} className="text-amber-400 shrink-0" />
              <span className="text-[12px] text-amber-400/90">
                {t('newProjectDialog.idfNotConfiguredHint')}
              </span>
              <button
                onClick={() => {
                  onClose();
                  onOpenSettings?.();
                }}
                className="text-[12px] text-blue-400 hover:text-blue-300 underline underline-offset-2 transition-colors"
              >
                {t('newProjectDialog.goToSettings')}
              </button>
            </div>
          )}
          <div className="flex items-center justify-end gap-3">
            <button
              onClick={onClose}
              className="px-5 py-2.5 text-[13px] font-medium text-text-secondary bg-white/5 border border-white/10 rounded-xl hover:bg-white/10 hover:text-text-primary hover:border-white/20 transition-all duration-200"
            >
              {t('newProjectDialog.cancel')}
            </button>
            <button
              onClick={handleCreate}
              disabled={creating || !projectName.trim() || !projectPath.trim() || !idfPath}
              className="px-6 py-2.5 text-[13px] font-semibold bg-gradient-to-r from-blue-500 to-blue-600 text-white rounded-xl hover:from-blue-600 hover:to-blue-700 hover:shadow-lg hover:shadow-blue-500/20 disabled:opacity-30 disabled:cursor-not-allowed transition-all duration-200 active:scale-[0.98] flex items-center gap-2"
            >
              {creating ? (
                <>
                  <Loader2 size={14} className="animate-spin" />
                  {t('newProjectDialog.creating')}
                </>
              ) : (
                <>
                  <Sparkles size={14} />
                  {t('newProjectDialog.create')}
                </>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}