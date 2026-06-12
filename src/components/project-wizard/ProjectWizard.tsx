/**
 * ProjectWizard - 项目创建向导
 *
 * 流程：项目信息 → 芯片型号 → 创建 (通过 idf.py create-project)
 * 全新设计主题：玻璃态卡片 + 渐变背景 + 动态光效
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X, ChevronRight, ChevronLeft, Check, Loader2, Sparkles, AlertTriangle, FolderOpen, Cpu, Layers } from 'lucide-react';
import { safeInvoke } from '../../lib/invoke';
import { useProjectStore, useSettingsStore } from '../../stores';

const CHIP_COLORS = [
  { gradient: 'from-blue-500/20 to-cyan-500/20', icon: '🟦' },
  { gradient: 'from-purple-500/20 to-pink-500/20', icon: '🟣' },
  { gradient: 'from-emerald-500/20 to-teal-500/20', icon: '🟢' },
  { gradient: 'from-orange-500/20 to-yellow-500/20', icon: '🟠' },
];

const STEP_ICONS = [FolderOpen, Cpu, Sparkles];

export function ProjectWizard({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();

  const STEPS = [
    { title: t('projectWizard.steps.projectInfo'), key: 'info', desc: t('projectWizard.steps.projectInfoDesc') },
    { title: t('projectWizard.steps.chipModel'), key: 'chip', desc: t('projectWizard.steps.chipModelDesc') },
  ];

  const CHIP_OPTIONS = [
    { id: 'ESP32', name: 'ESP32', desc: t('projectWizard.chips.ESP32'), ...CHIP_COLORS[0] },
    { id: 'ESP32-S3', name: 'ESP32-S3', desc: t('projectWizard.chips.ESP32-S3'), ...CHIP_COLORS[1] },
    { id: 'ESP32-C3', name: 'ESP32-C3', desc: t('projectWizard.chips.ESP32-C3'), ...CHIP_COLORS[2] },
    { id: 'ESP32-C6', name: 'ESP32-C6', desc: t('projectWizard.chips.ESP32-C6'), ...CHIP_COLORS[3] },
  ];

  const [step, setStep] = useState(0);
  const [projectName, setProjectName] = useState('my_esp_project');
  const [projectPath, setProjectPath] = useState('');
  const [chip, setChip] = useState('ESP32');
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { openProject } = useProjectStore();
  const { settings } = useSettingsStore();

  const idfPath = settings.idfPath || '';
  const hasIdf = !!idfPath;

  const handleCreate = async () => {
    if (!hasIdf) {
      setError(t('projectWizard.idfNotConfigured'));
      return;
    }
    setIsCreating(true);
    setError(null);
    try {
      const config = { name: projectName, path: projectPath, chip, idf_path: idfPath };
      const result = await safeInvoke<string>('create_project', { config });
      if (!result) {
        setError('No response from backend');
        return;
      }
      await openProject(result);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setIsCreating(false);
    }
  };

  const canProceed = () => {
    switch (step) {
      case 0: return projectName.trim() && projectPath.trim();
      case 1: return !!chip;
      default: return true;
    }
  };

  const progressPercent = ((step + 1) / STEPS.length) * 100;

  return (
    <div className="fixed inset-0 bg-black/70 backdrop-blur-lg flex items-center justify-center z-50 animate-fade-in">
      {/* 背景光效 */}
      <div className="absolute inset-0 overflow-hidden pointer-events-none">
        <div className="absolute top-1/4 left-1/4 w-[600px] h-[600px] bg-blue-500/10 rounded-full blur-[120px] animate-pulse-slow" />
        <div className="absolute bottom-1/4 right-1/4 w-[500px] h-[500px] bg-purple-500/10 rounded-full blur-[120px] animate-pulse-slow" style={{ animationDelay: '2s' }} />
      </div>

      <div className="relative bg-surface-elevated/90 backdrop-blur-xl border border-white/10 rounded-2xl w-[580px] max-h-[85vh] flex flex-col shadow-2xl shadow-black/40 animate-scale-in overflow-hidden">
        {/* 顶部渐变条 */}
        <div className="absolute top-0 inset-x-0 h-0.5 bg-gradient-to-r from-blue-500 via-purple-500 to-emerald-500" />

        {/* Header */}
        <div className="px-8 py-5 border-b border-white/5 flex items-center justify-between">
          <div>
            <h2 className="text-[17px] font-bold text-text-primary tracking-tight">
              {t('projectWizard.title')}
            </h2>
            <p className="text-[12px] text-text-tertiary mt-1">
              {STEPS[step].desc}
            </p>
          </div>
          <button
            onClick={onClose}
            className="p-2 rounded-lg text-text-tertiary hover:text-text-primary hover:bg-white/5 transition-all duration-200"
          >
            <X size={18} />
          </button>
        </div>

        {/* 进度条 */}
        <div className="h-1 bg-white/5">
          <div
            className="h-full bg-gradient-to-r from-blue-500 via-purple-500 to-emerald-500 rounded-full transition-all duration-500 ease-out"
            style={{ width: `${progressPercent}%` }}
          />
        </div>

        {/* Step indicators */}
        <div className="flex px-8 pt-5 pb-2">
          {STEPS.map((s, i) => {
            const StepIcon = STEP_ICONS[i];
            const isCompleted = i < step;
            const isCurrent = i === step;

            return (
              <div key={s.key} className="flex items-center flex-1">
                <div className="flex items-center gap-2.5">
                  <div
                    className={`
                      w-9 h-9 rounded-xl flex items-center justify-center shrink-0 transition-all duration-500
                      ${isCompleted
                        ? 'bg-gradient-to-br from-emerald-400 to-emerald-600 text-white shadow-lg shadow-emerald-500/25'
                        : isCurrent
                          ? 'bg-gradient-to-br from-blue-500 to-purple-600 text-white shadow-lg shadow-blue-500/25 ring-4 ring-blue-500/20'
                          : 'bg-white/5 text-text-disabled border border-white/10'
                      }
                    `}
                  >
                    {isCompleted ? <Check size={16} /> : <StepIcon size={16} />}
                  </div>
                  <div className="flex flex-col gap-0.5">
                    <span className={`text-[12px] font-semibold transition-colors duration-300 ${
                      isCurrent ? 'text-text-primary' : isCompleted ? 'text-emerald-400' : 'text-text-disabled'
                    }`}>
                      {s.title}
                    </span>
                    <span className="text-[10px] text-text-disabled">
                      {s.desc}
                    </span>
                  </div>
                </div>
                {i < STEPS.length - 1 && (
                  <div className="flex-1 mx-4">
                    <div className={`h-0.5 rounded-full transition-all duration-500 ${
                      i < step ? 'bg-gradient-to-r from-emerald-400 to-emerald-400' : 'bg-white/5'
                    }`} />
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-8 py-6">
          {step === 0 && (
            <div className="space-y-5 animate-slide-up">
              <InputGroup label={t('projectWizard.projectName')} icon={<Layers size={14} />}>
                <input
                  type="text"
                  value={projectName}
                  onChange={e => setProjectName(e.target.value)}
                  placeholder={t('projectWizard.projectNamePlaceholder')}
                  className="w-full px-4 py-3 text-[13px] bg-white/5 border border-white/10 rounded-xl text-text-primary placeholder:text-text-disabled/50 focus:outline-none focus:border-blue-500/50 focus:bg-white/[0.07] focus:ring-4 focus:ring-blue-500/10 transition-all duration-300"
                  autoFocus
                />
              </InputGroup>
              <InputGroup label={t('projectWizard.projectPath')} icon={<FolderOpen size={14} />}>
                <input
                  type="text"
                  value={projectPath}
                  onChange={e => setProjectPath(e.target.value)}
                  placeholder={t('projectWizard.projectPathPlaceholder')}
                  className="w-full px-4 py-3 text-[13px] bg-white/5 border border-white/10 rounded-xl text-text-primary placeholder:text-text-disabled/50 focus:outline-none focus:border-blue-500/50 focus:bg-white/[0.07] focus:ring-4 focus:ring-blue-500/10 transition-all duration-300 font-mono"
                />
              </InputGroup>

              {/* IDF 未配置警告 */}
              {!hasIdf && (
                <div className="flex items-start gap-3 p-4 bg-amber-500/5 border border-amber-500/20 rounded-xl">
                  <div className="p-1.5 rounded-lg bg-amber-500/10 shrink-0">
                    <AlertTriangle size={16} className="text-amber-400" />
                  </div>
                  <div className="text-[12px]">
                    <p className="font-semibold text-amber-400">{t('projectWizard.idfNotConfiguredTitle')}</p>
                    <p className="mt-1 text-text-tertiary">{t('projectWizard.idfNotConfiguredDesc')}</p>
                  </div>
                </div>
              )}
            </div>
          )}

          {step === 1 && (
            <div className="animate-slide-up">
              <p className="text-[12px] text-text-tertiary mb-4">
                {t('projectWizard.chipModelDesc')}
              </p>
              <div className="grid grid-cols-2 gap-3">
                {CHIP_OPTIONS.map(o => (
                  <ChipCard
                    key={o.id}
                    selected={chip === o.id}
                    onClick={() => setChip(o.id)}
                    title={o.name}
                    desc={o.desc}
                    icon={o.icon}
                    gradient={o.gradient}
                  />
                ))}
              </div>
            </div>
          )}

          {error && (
            <div className="mt-4 p-4 bg-red-500/5 border border-red-500/20 rounded-xl text-[12px] text-red-400 flex items-start gap-3">
              <div className="p-1 rounded bg-red-500/10 shrink-0 mt-0.5">
                <AlertTriangle size={14} />
              </div>
              {error}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="px-8 py-4 border-t border-white/5 flex justify-between bg-white/[0.02]">
          <button
            onClick={() => setStep(step - 1)}
            disabled={step === 0}
            className="flex items-center gap-1.5 px-5 py-2.5 text-[13px] font-medium bg-white/5 border border-white/10 rounded-xl text-text-secondary hover:text-text-primary hover:bg-white/10 hover:border-white/20 disabled:opacity-20 disabled:cursor-not-allowed transition-all duration-200"
          >
            <ChevronLeft size={15} />
            {t('projectWizard.previous')}
          </button>
          {step < STEPS.length - 1 ? (
            <button
              onClick={() => canProceed() && setStep(step + 1)}
              disabled={!canProceed()}
              className="flex items-center gap-1.5 px-6 py-2.5 text-[13px] font-semibold bg-gradient-to-r from-blue-500 to-blue-600 text-white rounded-xl hover:from-blue-600 hover:to-blue-700 hover:shadow-lg hover:shadow-blue-500/20 disabled:opacity-30 disabled:cursor-not-allowed transition-all duration-200 active:scale-[0.98]"
            >
              {t('projectWizard.next')}
              <ChevronRight size={15} />
            </button>
          ) : (
            <button
              onClick={handleCreate}
              disabled={!canProceed() || isCreating || !hasIdf}
              className="flex items-center gap-2 px-6 py-2.5 text-[13px] font-semibold bg-gradient-to-r from-emerald-500 to-emerald-600 text-white rounded-xl hover:from-emerald-600 hover:to-emerald-700 hover:shadow-lg hover:shadow-emerald-500/20 disabled:opacity-30 disabled:cursor-not-allowed transition-all duration-200 active:scale-[0.98]"
            >
              {isCreating ? (
                <>
                  <Loader2 size={15} className="animate-spin" />
                  {t('projectWizard.creating')}
                </>
              ) : (
                <>
                  <Sparkles size={15} />
                  {t('projectWizard.create')}
                </>
              )}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function InputGroup({ label, icon, children }: { label: string; icon: React.ReactNode; children: React.ReactNode }) {
  return (
    <div>
      <label className="flex items-center gap-1.5 text-[11px] font-semibold text-text-secondary mb-2 uppercase tracking-wider">
        {icon}
        {label}
      </label>
      {children}
    </div>
  );
}

function ChipCard({
  selected,
  onClick,
  title,
  desc,
  icon,
  gradient,
}: {
  selected: boolean;
  onClick: () => void;
  title: string;
  desc: string;
  icon: string;
  gradient: string;
}) {
  return (
    <div
      onClick={onClick}
      className={`
        relative p-4 rounded-xl cursor-pointer border transition-all duration-300 overflow-hidden group
        ${selected
          ? `border-blue-500/50 bg-gradient-to-br ${gradient} shadow-lg shadow-blue-500/10`
          : 'border-white/10 bg-white/[0.03] hover:border-white/20 hover:bg-white/[0.06] hover:shadow-md'
        }
      `}
    >
      {/* 选中状态光效 */}
      {selected && (
        <div className="absolute inset-0 bg-gradient-to-tr from-transparent via-white/[0.03] to-transparent" />
      )}

      <div className="relative flex items-center gap-3.5">
        <div className={`
          w-10 h-10 rounded-xl flex items-center justify-center text-lg shrink-0 transition-all duration-300
          ${selected
            ? 'bg-white/10 shadow-inner'
            : 'bg-white/5 group-hover:bg-white/10'
          }
        `}>
          {icon}
        </div>
        <div className="flex-1 min-w-0">
          <div className={`text-[13px] font-bold transition-colors duration-300 ${
            selected ? 'text-blue-400' : 'text-text-primary group-hover:text-text-primary'
          }`}>
            {title}
          </div>
          <div className="text-[11px] text-text-tertiary mt-0.5 leading-relaxed">
            {desc}
          </div>
        </div>
        <div className={`
          w-5 h-5 rounded-full border-2 shrink-0 flex items-center justify-center transition-all duration-300
          ${selected
            ? 'border-blue-400 bg-blue-400 shadow-sm shadow-blue-400/30'
            : 'border-white/20 group-hover:border-white/40'
          }
        `}>
          {selected && <Check size={11} className="text-white" strokeWidth={3} />}
        </div>
      </div>
    </div>
  );
}

export default ProjectWizard;