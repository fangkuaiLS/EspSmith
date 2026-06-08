/**
 * WelcomeScreen - 欢迎页 / 安装向导首页
 *
 * 用户首次打开或未打开项目时的主界面
 * 全新设计主题：玻璃态卡片 + 渐变背景 + 动态光效 + 粒子动画
 */

import { FolderOpen, FolderPlus, Star, Minus, Square, X, Copy } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { isTauri } from '../lib/invoke';
import { useState, useCallback } from 'react';

interface WelcomeScreenProps {
  onNewProject: () => void;
  onOpenFolder: () => void;
}

export function WelcomeScreen({ onNewProject, onOpenFolder }: WelcomeScreenProps) {
  const { t } = useTranslation();
  const githubUrl = 'https://github.com/fangkuaiLS/EspSmith';
  const [isMaximized, setIsMaximized] = useState(false);

  const handleMinimize = useCallback(async () => {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().minimize();
    }
  }, []);

  const handleMaximize = useCallback(async () => {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().toggleMaximize();
      setIsMaximized(await getCurrentWindow().isMaximized());
    }
  }, []);

  const handleClose = useCallback(async () => {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().close();
    }
  }, []);

  return (
    <div className="h-screen w-screen bg-surface-root flex flex-col overflow-hidden relative">
      {/* ===== 窗口标题栏 ===== */}
      <header data-tauri-drag-region className="h-11 shrink-0 flex items-center justify-end select-none z-50 relative">
        {isTauri() && (
          <div className="flex items-center">
            <button
              onClick={handleMinimize}
              title={t('window.minimize')}
              className="flex items-center justify-center w-11 h-11 text-text-tertiary hover:text-text-primary hover:bg-white/10 transition-colors"
            >
              <Minus size={13} />
            </button>
            <button
              onClick={handleMaximize}
              title={isMaximized ? t('window.restore') : t('window.maximize')}
              className="flex items-center justify-center w-11 h-11 text-text-tertiary hover:text-text-primary hover:bg-white/10 transition-colors"
            >
              {isMaximized ? <Copy size={12} /> : <Square size={11} />}
            </button>
            <button
              onClick={handleClose}
              title={t('window.close')}
              className="flex items-center justify-center w-11 h-11 text-text-tertiary hover:text-white hover:bg-red-500 transition-colors"
            >
              <X size={14} />
            </button>
          </div>
        )}
      </header>

      {/* ===== 背景层 ===== */}
      {/* 网格背景 */}
      <div
        className="absolute inset-0 opacity-[0.03]"
        style={{
          backgroundImage: `radial-gradient(circle, rgba(255,255,255,0.8) 1px, transparent 1px)`,
          backgroundSize: '40px 40px',
        }}
      />

      {/* 浮动光晕 */}
      <div className="absolute top-1/4 left-1/4 w-[600px] h-[600px] bg-blue-500/8 rounded-full blur-[140px] animate-pulse-slow" />
      <div className="absolute bottom-1/4 right-1/4 w-[500px] h-[500px] bg-purple-500/8 rounded-full blur-[140px] animate-pulse-slow" style={{ animationDelay: '1.5s' }} />
      <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[400px] h-[400px] bg-emerald-500/5 rounded-full blur-[120px] animate-pulse-slow" style={{ animationDelay: '3s' }} />

      {/* 顶部渐变光条 */}
      <div className="absolute top-0 inset-x-0 h-[1px] bg-gradient-to-r from-transparent via-blue-500/30 to-transparent" />
      <div className="absolute bottom-0 inset-x-0 h-[1px] bg-gradient-to-r from-transparent via-purple-500/30 to-transparent" />

      {/* ===== 主内容卡片 ===== */}
      <div className="relative z-10 flex-1 flex flex-col items-center justify-center gap-10 animate-scale-in">
        {/* Logo 区域 */}
        <div className="flex flex-col items-center gap-5 opacity-0 animate-[fadeSlideUp_0.6s_ease-out_0.1s_forwards]">
          {/* Logo 容器 - 带发光效果 */}
          <div className="relative">
            <div className="absolute inset-0 bg-blue-500/20 rounded-full blur-2xl scale-150 animate-pulse-slow" />
            <div className="relative w-24 h-24 rounded-2xl bg-gradient-to-br from-surface-elevated via-surface-elevated to-blue-500/5 border border-white/10 flex items-center justify-center shadow-2xl shadow-blue-500/10">
              <img src="/logo-b.png" alt="EspSmith" className="w-14 h-14" />
            </div>
          </div>

          {/* 标题 */}
          <div className="flex flex-col items-center gap-2">
            <h1 className="text-4xl font-bold tracking-tight">
              <span className="text-text-primary">Esp</span>
              <span className="bg-gradient-to-r from-blue-400 via-purple-400 to-emerald-400 bg-clip-text text-transparent">
                Smith
              </span>
            </h1>
            <p className="text-[15px] text-text-tertiary">
              {t('welcomeScreen.subtitle')}
            </p>
          </div>
        </div>

        {/* GitHub 链接 */}
        <a
          href={githubUrl || '#'}
          target="_blank"
          rel="noopener noreferrer"
          className="opacity-0 animate-[fadeSlideUp_0.5s_ease-out_0.4s_forwards] flex items-center gap-2 px-4 py-2 rounded-xl bg-white/[0.03] border border-white/5 hover:bg-white/[0.06] hover:border-white/10 transition-all duration-300 group"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="text-text-secondary group-hover:text-text-primary">
            <path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22"/>
          </svg>
          <span className="text-[12px] font-medium text-text-secondary group-hover:text-text-primary">
            GitHub
          </span>
          {githubUrl && (
            <>
              <span className="w-px h-3.5 bg-white/10" />
              <Star size={12} className="text-amber-400" />
              <span className="text-[11px] text-text-tertiary">--</span>
            </>
          )}
        </a>

        {/* 分割线 */}
        <div className="opacity-0 animate-[fadeSlideUp_0.4s_ease-out_0.55s_forwards] flex items-center gap-4 w-full max-w-md">
          <div className="flex-1 h-px bg-gradient-to-r from-transparent to-white/10" />
          <span className="text-[11px] text-text-disabled uppercase tracking-[0.2em]">
            {t('app.title')}
          </span>
          <div className="flex-1 h-px bg-gradient-to-l from-transparent to-white/10" />
        </div>

        {/* 操作按钮 */}
        <div className="opacity-0 animate-[fadeSlideUp_0.5s_ease-out_0.7s_forwards] flex gap-4">
          {/* 新建项目 */}
          <ActionCard
            icon={<FolderPlus size={22} />}
            title={t('welcomeScreen.newProject')}
            desc={t('welcomeScreen.newProjectDesc')}
            primary
            onClick={onNewProject}
          />

          {/* 打开文件夹 */}
          <ActionCard
            icon={<FolderOpen size={22} />}
            title={t('welcomeScreen.openFolder')}
            desc={t('welcomeScreen.openFolderDesc')}
            onClick={onOpenFolder}
          />
        </div>

        </div>
    </div>
  );
}

/** 操作卡片按钮 */
function ActionCard({
  icon,
  title,
  desc,
  primary,
  onClick,
}: {
  icon: React.ReactNode;
  title: string;
  desc: string;
  primary?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`
        group relative flex flex-col items-center gap-3.5 w-[220px] p-7 rounded-2xl border transition-all duration-300 cursor-pointer
        ${primary
          ? 'bg-gradient-to-br from-blue-500/10 to-purple-500/10 border-blue-500/20 hover:border-blue-500/40 hover:from-blue-500/15 hover:to-purple-500/15 hover:shadow-lg hover:shadow-blue-500/10'
          : 'bg-white/[0.03] border-white/10 hover:border-white/20 hover:bg-white/[0.06] hover:shadow-md'
        }
      `}
    >
      {/* 选中光效 */}
      <div className={`
        absolute inset-0 rounded-2xl opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none
        ${primary
          ? 'bg-gradient-to-t from-blue-500/5 via-transparent to-transparent'
          : 'bg-gradient-to-t from-white/[0.03] via-transparent to-transparent'
        }
      `} />

      <div className={`
        relative w-14 h-14 rounded-xl flex items-center justify-center transition-all duration-300 group-hover:scale-110
        ${primary
          ? 'bg-gradient-to-br from-blue-500/20 to-purple-500/20 text-blue-400 group-hover:text-blue-300 group-hover:shadow-lg group-hover:shadow-blue-500/20'
          : 'bg-white/5 text-text-tertiary group-hover:text-text-primary group-hover:bg-white/10'
        }
      `}>
        {icon}
      </div>

      <div className="relative flex flex-col items-center gap-1">
        <span className="text-[15px] font-semibold text-text-primary">
          {title}
        </span>
        <span className="text-[11px] text-text-tertiary leading-relaxed text-center">
          {desc}
        </span>
      </div>
    </button>
  );
}

export default WelcomeScreen;