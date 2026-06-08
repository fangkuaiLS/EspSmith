/**
 * SettingsDialog - 设置面板组件 (Codex-inspired)
 */

import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { X, FolderOpen, RefreshCw, Check, AlertTriangle, Loader2, Cpu, Bot, Shield, Globe, Download, ChevronDown, ExternalLink, Sparkles } from 'lucide-react';
import { useSettingsStore } from '../../stores';
import { AppSettings } from '../../types';
import { getCurrentLanguage, switchLanguage, translateBackendString } from '../../i18n';
import { isTauri, safeInvoke } from '../../lib/invoke';

interface SettingsDialogProps {
  onClose: () => void;
}

type SettingsTab = 'idf' | 'ai' | 'security' | 'language' | 'about';

export function SettingsDialog({ onClose }: SettingsDialogProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<SettingsTab>('idf');
  const { settings, idfStatus, setSettings, detectIDF, validateIDFPath, isLoading } = useSettingsStore();
  const [localSettings, setLocalSettings] = useState<AppSettings>({ ...settings });
  const [idfValidationError, setIdfValidationError] = useState<string | null>(null);
  const [isValidating, setIsValidating] = useState(false);
  const [saved, setSaved] = useState(false);
  const [currentLang, setCurrentLang] = useState(getCurrentLanguage());
  const [codewhaleStatus, setCodewhaleStatus] = useState<'local' | 'system' | 'missing' | 'checking'>('checking');
  const [isInstallingCodewhale, setIsInstallingCodewhale] = useState(false);
  const [codewhaleInstallError, setCodewhaleInstallError] = useState<string | null>(null);
  const [pythonValidating, setPythonValidating] = useState(false);
  const [pythonValidationResult, setPythonValidationResult] = useState<'success' | 'error' | null>(null);
  const [pythonVersion, setPythonVersion] = useState<string | null>(null);
  const [pythonValidationError, setPythonValidationError] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState<'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error' | 'up-to-date'>('idle');
  const [updateInfo, setUpdateInfo] = useState<{ version: string; body?: string } | null>(null);
  const [updateProgress, setUpdateProgress] = useState<number>(0);
  const [updateError, setUpdateError] = useState<string | null>(null);

  useEffect(() => { detectIDF(); }, [detectIDF]);

  const checkCodewhaleStatus = async () => {
    setCodewhaleStatus('checking');
    const status = await safeInvoke<string>('check_codewhale_status');
    setCodewhaleStatus((status === 'local' || status === 'system' || status === 'missing') ? status : 'missing');
  };

  useEffect(() => { checkCodewhaleStatus(); }, []);

  const handleInstallCodewhale = async () => {
    setIsInstallingCodewhale(true);
    setCodewhaleInstallError(null);
    try {
      const result = await safeInvoke<string>('setup_codewhale');
      if (result === 'installed' || result === 'already_installed') {
        setCodewhaleStatus('local');
      } else {
        setCodewhaleInstallError('unknown_result');
      }
    } catch (err: any) {
      setCodewhaleInstallError(translateBackendString(err?.toString() || 'install_failed'));
    } finally {
      setIsInstallingCodewhale(false);
    }
  };

  const tabs: { key: SettingsTab; label: string; icon: React.ComponentType<{ size?: number | string; className?: string }> }[] = [
    { key: 'idf', label: 'ESP-IDF', icon: Cpu },
    { key: 'ai', label: t('settings.aiConfig'), icon: Bot },
    { key: 'security', label: t('settings.security'), icon: Shield },
    { key: 'language', label: t('settings.language'), icon: Globe },
    { key: 'about', label: t('settings.about'), icon: Sparkles },
  ];

  const handleSave = () => {
    setSettings(localSettings);
    setSaved(true);
    setTimeout(() => {
      setSaved(false);
      onClose();
    }, 600);
  };

  const handleBrowseIDF = async () => {
    if (isTauri()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ directory: true, multiple: false, title: 'Select ESP-IDF Directory' });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          setLocalSettings({ ...localSettings, idfPath: path });
          await validateIDFPathFunc(path);
        }
      } catch {
        // 降级到手动输入
        goManualInput();
      }
    } else {
      goManualInput();
    }
  };

  const goManualInput = () => {
    const path = prompt('Enter ESP-IDF directory path:', 'C:\\Espressif\\frameworks\\esp-idf');
    if (path) {
      setLocalSettings({ ...localSettings, idfPath: path });
      validateIDFPathFunc(path);
    }
  };

  const validateIDFPathFunc = async (path: string) => {
    if (!path) { setIdfValidationError(null); return; }
    setIsValidating(true);
    setIdfValidationError(null);
    try {
      await validateIDFPath(path);
    } catch (err) {
      setIdfValidationError(err as string);
    } finally {
      setIsValidating(false);
    }
  };

  const handleLanguageChange = (lang: 'zh' | 'en') => {
    switchLanguage(lang);
    setCurrentLang(lang);
  };

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 animate-fade-in">
      <div className="bg-surface-elevated border border-border-default rounded-xl w-[680px] max-h-[85vh] flex flex-col shadow-xl animate-scale-in">
        {/* Header */}
        <div className="px-6 py-4 border-b border-border-default flex items-center justify-between">
          <div>
            <h2 className="text-[16px] font-semibold">{t('settings.title')}</h2>
            <p className="text-[12px] text-text-tertiary mt-0.5">{t('settings.subtitle')}</p>
          </div>
          <button onClick={onClose} className="p-1.5 rounded-lg text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors">
            <X size={18} />
          </button>
        </div>

        {/* Tabs + Content */}
        <div className="flex-1 flex min-h-0">
          {/* Side tabs */}
          <div className="w-[180px] border-r border-border-default p-3 flex flex-col gap-1 shrink-0">
            {tabs.map(({ key, label, icon: Icon }) => (
              <button
                key={key}
                onClick={() => setActiveTab(key)}
                className={`flex items-center gap-2.5 px-3 py-2 rounded-lg text-[13px] font-medium transition-all ${
                  activeTab === key
                    ? 'bg-accent-muted text-accent'
                    : 'text-text-tertiary hover:text-text-primary hover:bg-surface-hover'
                }`}
              >
                <Icon size={15} />
                {label}
              </button>
            ))}
            <div className="flex-1" />
            <a
              href="https://my.feishu.cn/wiki/Lc1NwpWdbih8TGk62aacShlonWf"
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-2.5 px-3 py-2 rounded-lg text-[13px] font-medium text-text-tertiary hover:text-accent hover:bg-surface-hover transition-all"
            >
              <ExternalLink size={15} />
              {t('settings.helpDoc')}
            </a>
            <a
              href="https://github.com/fangkuaiLS/EspSmith"
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-2.5 px-3 py-2 rounded-lg text-[13px] font-medium text-text-tertiary hover:text-accent hover:bg-surface-hover transition-all"
            >
              <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/></svg>
              GitHub
            </a>
          </div>

          {/* Content */}
          <div className="flex-1 overflow-y-auto p-6">
            {activeTab === 'idf' && (
              <div className="space-y-5">
                <h3 className="text-[14px] font-semibold flex items-center gap-2">
                  {t('settings.espIdfConfig')}
                  <a
                    href="https://my.feishu.cn/wiki/QnUmwpFSFiHKWCkqa0DcAUEFnUb"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-[12px] font-normal text-accent hover:underline inline-flex items-center gap-1"
                  >
                    {t('settings.configGuide')}
                    <ExternalLink size={11} />
                  </a>
                </h3>

                <div className="flex items-center gap-2">
                  <button
                    onClick={async () => {
                      await detectIDF();
                      const detected = useSettingsStore.getState().idfStatus.detected;
                      if (detected) {
                        setLocalSettings({ ...localSettings, idfPath: detected.idf_path });
                        await validateIDFPathFunc(detected.idf_path);
                      }
                    }}
                    disabled={isLoading}
                    className="flex items-center gap-2 px-4 py-2 text-[12px] font-medium bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors disabled:opacity-50"
                  >
                    {isLoading ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
                    {isLoading ? t('settings.detecting') : '检测IDF目录'}
                  </button>
                  <span className="text-[11px] text-text-tertiary">自动检测系统中的 ESP-IDF 安装</span>
                </div>

                <InputGroup label={t('settings.idfPath')}>
                  <div className="flex gap-2">
                    <input
                      type="text"
                      value={localSettings.idfPath || ''}
                      onChange={(e) => setLocalSettings({ ...localSettings, idfPath: e.target.value })}
                      onBlur={(e) => validateIDFPathFunc(e.target.value)}
                      placeholder="e.g., C:\\Espressif\\frameworks\\esp-idf"
                      className="flex-1 px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-lg text-text-primary placeholder:text-text-disabled focus:outline-none font-mono"
                    />
                    <button onClick={handleBrowseIDF} className="flex items-center gap-1.5 px-3 py-2 text-[12px] bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors">
                      <FolderOpen size={14} />
                      {t('settings.browse')}
                    </button>
                  </div>
                  {isValidating && <p className="text-[11px] text-text-tertiary mt-1">{t('settings.validating')}</p>}
                  {idfValidationError && <p className="text-[11px] text-error mt-1">{idfValidationError}</p>}
                  {idfStatus.userConfigured && !idfValidationError && (
                    <p className="text-[11px] text-success mt-1">{t('settings.validIDF', { version: idfStatus.userConfigured.version })}</p>
                  )}
                </InputGroup>

                {idfStatus.userConfigured?.python_path && (
                  <div className="flex items-center gap-2 p-2 bg-surface-overlay border border-border-subtle rounded-lg">
                    <Check size={14} className="text-success shrink-0" />
                    <div className="flex-1 min-w-0">
                      <p className="text-[12px] text-text-secondary">Python: <span className="font-mono text-text-primary">{idfStatus.userConfigured.python_path}</span></p>
                    </div>
                  </div>
                )}

                <details className="group">
                  <summary className="flex items-center gap-1.5 text-[12px] text-text-tertiary cursor-pointer hover:text-text-secondary transition-colors select-none">
                    <ChevronDown size={12} className="transition-transform group-open:rotate-180" />
                    高级：手动配置 Python 路径
                  </summary>
                  <div className="mt-3 space-y-3 pl-4 border-l-2 border-border-subtle">
                    <div className="flex gap-2">
                      <input
                        type="text"
                        value={localSettings.pythonPath || ''}
                        onChange={(e) => setLocalSettings({ ...localSettings, pythonPath: e.target.value })}
                        placeholder="C:\\Espressif\\tools\\python\\v5.5.4\\venv\\Scripts\\python.exe"
                        className="flex-1 px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-lg text-text-primary placeholder:text-text-disabled focus:outline-none font-mono"
                      />
                      <button
                        onClick={async () => {
                          if (isTauri()) {
                            try {
                              const { open } = await import('@tauri-apps/plugin-dialog');
                              const selected = await open({ directory: false, multiple: false, title: '选择 Python', filters: [{ name: 'Python', extensions: ['exe'] }] });
                              if (selected) { const p = Array.isArray(selected) ? selected[0] : selected; setLocalSettings({ ...localSettings, pythonPath: p }); }
                            } catch {}
                          }
                        }}
                        className="flex items-center gap-1.5 px-3 py-2 text-[12px] bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors"
                      >
                        <FolderOpen size={14} />
                        {t('settings.browse')}
                      </button>
                    </div>
                    <div className="flex gap-2">
                      <button
                        onClick={async () => {
                          const pyPath = localSettings.pythonPath;
                          if (!pyPath) return;
                          setPythonValidating(true);
                          try {
                            const version = await safeInvoke<string>('validate_python_path', { path: pyPath });
                            setPythonValidationResult('success');
                            setPythonVersion(version);
                            setPythonValidationError(null);
                          } catch (err: any) {
                            setPythonValidationResult('error');
                            setPythonValidationError(err?.toString() || '验证失败');
                          } finally { setPythonValidating(false); }
                        }}
                        disabled={pythonValidating || !localSettings.pythonPath}
                        className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary disabled:opacity-50"
                      >
                        {pythonValidating ? <Loader2 size={12} className="animate-spin" /> : <Check size={12} />}
                        验证
                      </button>
                      {pythonValidationResult === 'success' && <span className="text-[11px] text-success self-center">✓ {pythonVersion}</span>}
                      {pythonValidationResult === 'error' && <span className="text-[11px] text-error self-center">{pythonValidationError}</span>}
                    </div>
                  </div>
                </details>

                {idfStatus.active && (
                  <div className="p-3 bg-accent-muted border border-accent/20 rounded-lg">
                    <p className="text-[12px] font-medium text-accent">{t('settings.activeIDF')}</p>
                    <p className="text-[11px] text-text-tertiary mt-1 font-mono">{idfStatus.active.idf_path}</p>
                  </div>
                )}
              </div>
            )}

            {activeTab === 'ai' && (
              <div className="space-y-5">
                <h3 className="text-[14px] font-semibold">{t('settings.aiConfig')}</h3>
                <p className="text-[12px] text-text-tertiary">模型选择请在聊天面板顶部的下拉菜单中进行</p>

                <InputGroup label={t('settings.deepseekApiKey')}>
                  <input
                    type="password"
                    value={localSettings.deepseekApiKey || ''}
                    onChange={(e) => setLocalSettings({ ...localSettings, deepseekApiKey: e.target.value })}
                    placeholder="sk-..."
                    className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-lg text-text-primary placeholder:text-text-disabled focus:outline-none font-mono"
                  />
                </InputGroup>

                <InputGroup label={t('settings.ollamaEndpoint')}>
                  <input
                    type="text"
                    value={localSettings.ollamaEndpoint || ''}
                    onChange={(e) => setLocalSettings({ ...localSettings, ollamaEndpoint: e.target.value })}
                    placeholder="http://localhost:11434"
                    className="w-full px-3 py-2 text-[13px] bg-surface-overlay border border-border-subtle rounded-lg text-text-primary placeholder:text-text-disabled focus:outline-none font-mono"
                  />
                </InputGroup>

                <div className="border-t border-border-subtle pt-5">
                  <h3 className="text-[13px] font-semibold mb-3">{t('settings.codewhaleConfig')}</h3>

                  <div className="flex items-center justify-between p-3 bg-surface-overlay rounded-lg border border-border-subtle">
                    <div className="flex items-center gap-2">
                      {codewhaleStatus === 'checking' ? (
                        <Loader2 size={14} className="text-text-tertiary animate-spin" />
                      ) : codewhaleStatus === 'local' ? (
                        <Check size={14} className="text-success" />
                      ) : codewhaleStatus === 'system' ? (
                        <Check size={14} className="text-success" />
                      ) : (
                        <AlertTriangle size={14} className="text-warning" />
                      )}
                      <span className="text-[13px] text-text-secondary">
                        {codewhaleStatus === 'checking'
                          ? '...'
                          : codewhaleStatus === 'local'
                          ? t('settings.codewhaleInstalled')
                          : codewhaleStatus === 'system'
                          ? t('settings.codewhaleSystemInstalled')
                          : t('settings.codewhaleNotInstalled')}
                      </span>
                    </div>
                    <button
                      onClick={checkCodewhaleStatus}
                      className="flex items-center gap-1 px-2 py-1 text-[11px] bg-surface-hover border border-border-subtle rounded-md text-text-tertiary hover:text-text-primary transition-colors"
                    >
                      <RefreshCw size={11} />
                      {t('settings.refreshDetection')}
                    </button>
                  </div>

                  {codewhaleStatus === 'missing' && (
                    <div className="mt-3">
                      <p className="text-[12px] text-text-tertiary mb-2">{t('settings.codewhaleNotInstalledDesc')}</p>
                      <button
                        onClick={handleInstallCodewhale}
                        disabled={isInstallingCodewhale}
                        className={`flex items-center gap-2 px-4 py-2 text-[12px] font-medium rounded-lg transition-all ${
                          isInstallingCodewhale
                            ? 'bg-surface-overlay border border-border-subtle text-text-tertiary cursor-not-allowed'
                            : 'bg-accent text-white hover:bg-accent-hover'
                        }`}
                      >
                        {isInstallingCodewhale ? (
                          <Loader2 size={14} className="animate-spin" />
                        ) : (
                          <Download size={14} />
                        )}
                        {isInstallingCodewhale ? t('settings.installingCodewhale') : t('settings.installCodewhale')}
                      </button>
                      {codewhaleInstallError && (
                        <p className="text-[11px] text-error mt-2">{t('settings.codewhaleInstallFailed')}</p>
                      )}
                    </div>
                  )}

                  {codewhaleStatus === 'local' && !codewhaleInstallError && (
                    <p className="text-[11px] text-success mt-2">{t('settings.codewhaleInstallSuccess')}</p>
                  )}
                </div>
              </div>
            )}

            {activeTab === 'security' && (
              <div className="space-y-5">
                <h3 className="text-[14px] font-semibold">{t('settings.security')}</h3>
                <label className="flex items-start gap-3 p-4 bg-surface-overlay rounded-lg border border-border-subtle cursor-pointer hover:border-border-default transition-colors">
                  <input
                    type="checkbox"
                    checked={localSettings.reviewMode}
                    onChange={(e) => setLocalSettings({ ...localSettings, reviewMode: e.target.checked })}
                    className="w-4 h-4 mt-0.5 rounded accent-accent"
                  />
                  <div className="flex-1">
                    <p className="text-[13px] font-medium">{t('settings.reviewMode')}</p>
                  </div>
                </label>
              </div>
            )}

            {activeTab === 'language' && (
              <div className="space-y-5">
                <h3 className="text-[14px] font-semibold">{t('settings.language')}</h3>
                <div className="space-y-2">
                  <button
                    onClick={() => handleLanguageChange('zh')}
                    className={`w-full flex items-center gap-3 p-3 rounded-lg border transition-all ${
                      currentLang === 'zh'
                        ? 'border-accent bg-accent-muted text-accent'
                        : 'border-border-subtle bg-surface-overlay text-text-secondary hover:border-border-default'
                    }`}
                  >
                    <span className="text-lg">🇨🇳</span>
                    <div className="text-left">
                      <p className="text-[13px] font-medium">{t('settings.chinese')}</p>
                    </div>
                    {currentLang === 'zh' && <Check size={14} className="ml-auto" />}
                  </button>
                  <button
                    onClick={() => handleLanguageChange('en')}
                    className={`w-full flex items-center gap-3 p-3 rounded-lg border transition-all ${
                      currentLang === 'en'
                        ? 'border-accent bg-accent-muted text-accent'
                        : 'border-border-subtle bg-surface-overlay text-text-secondary hover:border-border-default'
                    }`}
                  >
                    <span className="text-lg">🇺🇸</span>
                    <div className="text-left">
                      <p className="text-[13px] font-medium">{t('settings.english')}</p>
                    </div>
                    {currentLang === 'en' && <Check size={14} className="ml-auto" />}
                  </button>
                </div>
              </div>
            )}

            {activeTab === 'about' && (
              <div className="space-y-5">
                <h3 className="text-[14px] font-semibold">{t('settings.about')}</h3>

                {/* 版本信息 */}
                <div className="p-4 bg-surface-overlay rounded-lg border border-border-subtle">
                  <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-lg bg-accent/10 flex items-center justify-center text-accent font-bold text-[16px]">ES</div>
                    <div>
                      <p className="text-[14px] font-semibold">EspSmith</p>
                      <p className="text-[12px] text-text-tertiary font-mono">v0.1.0</p>
                    </div>
                  </div>
                  <p className="text-[12px] text-text-tertiary mt-3">{t('settings.aboutDesc')}</p>
                </div>

                {/* 检查更新 */}
                <div className="p-4 bg-surface-overlay rounded-lg border border-border-subtle">
                  <div className="flex items-center justify-between">
                    <div>
                      <p className="text-[13px] font-medium">{t('settings.checkUpdate')}</p>
                      <p className="text-[11px] text-text-tertiary mt-0.5">
                        {updateStatus === 'up-to-date' && t('settings.alreadyLatest')}
                        {updateStatus === 'available' && t('settings.newVersionAvailable', { version: updateInfo?.version })}
                        {updateStatus === 'checking' && t('settings.checkingUpdate')}
                        {updateStatus === 'downloading' && t('settings.downloadingUpdate', { percent: updateProgress })}
                        {updateStatus === 'ready' && t('settings.updateReady')}
                        {updateStatus === 'error' && (t('settings.updateError') + (updateError ? `: ${updateError}` : ''))}
                        {updateStatus === 'idle' && t('settings.checkUpdateDesc')}
                      </p>
                    </div>
                    {updateStatus === 'available' && (
                      <button
                        onClick={async () => {
                          try {
                            setUpdateStatus('downloading');
                            setUpdateProgress(0);
                            const { check } = await import('@tauri-apps/plugin-updater');
                            const { relaunch } = await import('@tauri-apps/plugin-process');
                            const update = await check();
                            if (update) {
                              await update.downloadAndInstall((event) => {
                                switch (event.event) {
                                  case 'Started':
                                    break;
                                  case 'Progress':
                                    setUpdateProgress(prev => Math.min(prev + 5, 95));
                                    break;
                                  case 'Finished':
                                    setUpdateProgress(100);
                                    break;
                                }
                              });
                              setUpdateStatus('ready');
                              await relaunch();
                            }
                          } catch (err: any) {
                            setUpdateStatus('error');
                            setUpdateError(err?.toString() || 'Unknown error');
                          }
                        }}
                        className="flex items-center gap-1.5 px-4 py-2 text-[12px] font-medium bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors"
                      >
                        <Download size={14} />
                        {t('settings.installUpdate')}
                      </button>
                    )}
                    {(updateStatus === 'idle' || updateStatus === 'up-to-date' || updateStatus === 'error' || updateStatus === 'checking') && (
                      <button
                        onClick={async () => {
                          if (!isTauri()) return;
                          if (updateStatus === 'checking') return;
                          try {
                            setUpdateStatus('checking');
                            setUpdateError(null);
                            const { check } = await import('@tauri-apps/plugin-updater');
                            const update = await check();
                            if (update?.available) {
                              setUpdateStatus('available');
                              setUpdateInfo({ version: update.version, body: update.body || undefined });
                            } else if (update) {
                              setUpdateStatus('up-to-date');
                            } else {
                              setUpdateStatus('error');
                              setUpdateError('Check returned null');
                            }
                          } catch (err: any) {
                            setUpdateStatus('error');
                            setUpdateError(err?.toString() || 'Unknown error');
                          }
                        }}
                        disabled={updateStatus === 'checking'}
                        className="flex items-center gap-1.5 px-4 py-2 text-[12px] font-medium bg-surface-hover border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary disabled:opacity-50 transition-colors"
                      >
                        {updateStatus === 'checking' ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
                        {t('settings.checkNow')}
                      </button>
                    )}
                    {updateStatus === 'downloading' && (
                      <div className="w-32 h-2 bg-surface-hover rounded-full overflow-hidden">
                        <div className="h-full bg-accent rounded-full transition-all duration-300" style={{ width: `${updateProgress}%` }} />
                      </div>
                    )}
                  </div>
                  {updateStatus === 'available' && updateInfo?.body && (
                    <div className="mt-3 p-3 bg-surface-hover rounded-lg">
                      <p className="text-[11px] text-text-tertiary mb-1">{t('settings.releaseNotes')}</p>
                      <p className="text-[12px] text-text-secondary whitespace-pre-wrap">{updateInfo.body}</p>
                    </div>
                  )}
                </div>

                {/* 链接 */}
                <div className="space-y-2">
                  <a
                    href="https://github.com/fangkuaiLS/EspSmith"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex items-center gap-3 p-3 rounded-lg border border-border-subtle bg-surface-overlay text-text-secondary hover:text-accent hover:border-accent/30 transition-all"
                  >
                    <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/></svg>
                    <div className="flex-1">
                      <p className="text-[13px] font-medium">GitHub</p>
                      <p className="text-[11px] text-text-tertiary">fangkuaiLS/EspSmith</p>
                    </div>
                    <ExternalLink size={14} className="text-text-tertiary" />
                  </a>
                  <a
                    href="https://github.com/fangkuaiLS/EspSmith/issues"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex items-center gap-3 p-3 rounded-lg border border-border-subtle bg-surface-overlay text-text-secondary hover:text-accent hover:border-accent/30 transition-all"
                  >
                    <AlertTriangle size={16} />
                    <div className="flex-1">
                      <p className="text-[13px] font-medium">{t('settings.reportIssue')}</p>
                      <p className="text-[11px] text-text-tertiary">{t('settings.reportIssueDesc')}</p>
                    </div>
                    <ExternalLink size={14} className="text-text-tertiary" />
                  </a>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="px-6 py-4 border-t border-border-default flex items-center justify-between gap-2">
          <span className="text-[11px] text-text-tertiary font-mono tracking-wide">EspSmith v0.1.0</span>
          <div className="flex gap-2">
            <button onClick={onClose} className="px-4 py-2 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors">
              {t('settings.cancel')}
            </button>
            <button
              onClick={handleSave}
              className={`flex items-center gap-1.5 px-5 py-2 text-[12px] font-medium rounded-lg transition-all ${
                saved ? 'bg-success text-white' : 'bg-accent text-white hover:bg-accent-hover'
              }`}
            >
              {saved ? <Check size={14} /> : null}
              {saved ? t('common.saved') : t('settings.save')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function InputGroup({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-[11px] font-medium text-text-tertiary mb-1.5 uppercase tracking-wider">{label}</label>
      {children}
    </div>
  );
}

export default SettingsDialog;