/**
 * EspSmith - AI驱动的ESP32开发环境
 *
 * Codex-inspired 四区域布局：
 * - 左侧栏：文件树 + 硬件商店入口 + Git 面板
 * - 中部左：代码编辑器（多标签页）
 * - 中部右：AI 聊天界面
 * - 底部：构建输出 / 串口监视器 / 调试控制台（可折叠）
 */

import { useState, useRef, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Cpu, FolderOpen, FilePlus, Save, Hammer, Zap, Radio,
  Settings, ChevronDown, PanelLeft,
  Bug, Terminal, GitBranch, Loader2, FolderPlus,
  Minus, Square, Copy, X, SlidersHorizontal, Trash2, BarChart3,
  Eraser, Play, HeartPulse, Usb, Download
} from 'lucide-react';
import { Panel, PanelGroup, PanelResizeHandle, ImperativePanelHandle } from 'react-resizable-panels';
import { FileTree } from './components/filetree';
import { CodeEditor } from './components/editor';
import { ChatPanel } from './components/chat';
import { HardwareStore } from './components/hardware';
import { SettingsDialog } from './components/settings';
import { GitPanel } from './components/git';
import { NewProjectDialog } from './components/NewProjectDialog';
import { InputDialog } from './components/ui/InputDialog';
import { DebugPanel, BuildOutputPanel, SerialMonitorPanel } from './components/debug';
import { ToastContainer, showToast } from './components/ui/Toast';
import { GlobalSearchPanel } from './components/search/GlobalSearchPanel';
import { QuickOpenDialog } from './components/quick-open/QuickOpenDialog';
import { WelcomeScreen } from './components/WelcomeScreen';
import { SdkConfigEditor } from './components/sdkconfig';
import { useProjectStore, useFileStore, useChatStore, useHardwareStore, useSettingsStore } from './stores';
import { safeInvoke, isTauri } from './lib/invoke';
import { useBuildOutput } from './hooks/useBuildOutput';
import { useSerialMonitor } from './hooks/useSerialMonitor';
import type { SerialPortInfo, ChipTargetInfo, ConnectionInfo } from './types';

type LeftPanel = 'files' | 'hardware';
type BottomTab = 'build' | 'serial' | 'debug';
type ViewMode = 'auto' | 'code';

function getInitialViewMode(): ViewMode {
  try {
    const stored = localStorage.getItem('espsmith:viewMode');
    if (stored === 'auto' || stored === 'code') return stored;
  } catch {}
  return 'auto';
}

function App() {
  const { t } = useTranslation();
  const [viewMode, setViewMode] = useState<ViewMode>(getInitialViewMode());
  const [leftPanel, setLeftPanel] = useState<LeftPanel>('files');
  const [activeBottomTab, setActiveBottomTab] = useState<BottomTab>('build');
  const [showSettings, setShowSettings] = useState(false);
  const [showNewProject, setShowNewProject] = useState(false);
  const [showGlobalSearch, setShowGlobalSearch] = useState(false);
  const [showQuickOpen, setShowQuickOpen] = useState(false);
  const [showSdkConfig, setShowSdkConfig] = useState(false);

  const [updateBanner, setUpdateBanner] = useState<{ version: string } | null>(null);
  const [updateBannerDownloading, setUpdateBannerDownloading] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    (async () => {
      try {
        const { check } = await import('@tauri-apps/plugin-updater');
        const update = await check();
        if (update?.available) {
          setUpdateBanner({ version: update.version });
        }
      } catch {}
    })();
  }, []);

  const isAutoMode = viewMode === 'auto';

  useEffect(() => {
    const splash = document.getElementById('splash');
    if (splash) {
      splash.classList.add('fade-out');
      const timer = setTimeout(() => splash.remove(), 500);
      return () => clearTimeout(timer);
    }
  }, []);

  const handleViewModeChange = useCallback((mode: ViewMode) => {
    setViewMode(mode);
    try { localStorage.setItem('espsmith:viewMode', mode); } catch {}
  }, []);

  type DialogType = 'newFile' | 'newFolder' | null;
  const [dialogType, setDialogType] = useState<DialogType>(null);
  const showInputDialog = dialogType !== null;
  const { currentProject, openProject, restoreCurrentCache } = useProjectStore();
  const { activeTabId, saveFile, openFile, cursorLine, cursorColumn, editorLanguage } = useFileStore();
const { loadConfig, detectConnection, connectionMode, connectionInfo } = useHardwareStore();
const { idfStatus, settings, detectIDF } = useSettingsStore();

  const leftPanelRef = useRef<ImperativePanelHandle>(null);
  const rightPanelRef = useRef<ImperativePanelHandle>(null);
  const bottomPanelRef = useRef<ImperativePanelHandle>(null);

  const [bottomCollapsed, setBottomCollapsed] = useState(false);
  const [autoHardwareCollapsed, setAutoHardwareCollapsed] = useState(false);
  const autoHardwarePanelRef = useRef<ImperativePanelHandle>(null);

  const [isWindowMaximized, setIsWindowMaximized] = useState(false);

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
      setIsWindowMaximized(await getCurrentWindow().isMaximized());
    }
  }, []);

  const handleClose = useCallback(async () => {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      await getCurrentWindow().close();
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const window = getCurrentWindow();
        setIsWindowMaximized(await window.isMaximized());
        const unsub = await window.onResized(() => {
          window.isMaximized().then(setIsWindowMaximized);
        });
        unlisten = unsub;
      } catch {}
    })();
    return () => { unlisten?.(); };
  }, []);

  const [availablePorts, setAvailablePorts] = useState<SerialPortInfo[]>([]);

  const [chipTargets, setChipTargets] = useState<ChipTargetInfo[]>([]);
  const [selectedChip, setSelectedChip] = useState<string>('');
  const [selectedPort, setSelectedPort] = useState<string>('');
  const selectedChipRef = useRef(selectedChip);
  selectedChipRef.current = selectedChip;
  const selectedPortRef = useRef(selectedPort);
  selectedPortRef.current = selectedPort;

  const autoSelectLock = useRef(false);

  useEffect(() => {
    (async () => {
      const idfPath = getIdfPath();
      if (!idfPath) return;
      try {
        const targets = await safeInvoke<ChipTargetInfo[]>('idf_get_supported_targets', { idfPath });
        if (targets && targets.length > 0) {
          setChipTargets(targets);
        }
      } catch {}
    })();
  }, [idfStatus.active?.idf_path, settings.idfPath]);

  useEffect(() => {
    const handleBeforeUnload = () => {
      const { currentProject } = useProjectStore.getState();
      if (currentProject?.path) {
        try {
          const fileState = useFileStore.getState();
          const chatState = useChatStore.getState();
          const cacheData = {
            tabs: fileState.tabs.map((t) => ({ path: t.path })),
            activeTabPath: (() => {
              const active = fileState.tabs.find((t) => t.id === fileState.activeTabId);
              return active?.path ?? null;
            })(),
            chatMessages: chatState.messages,
            version: 1,
          };
          const cachePath = currentProject.path.replace(/[\\/]+$/, '') + '\\.esp-ai-cache.json';
          localStorage.setItem(`cache:${cachePath}`, JSON.stringify(cacheData));
        } catch {}
      }
    };
    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => window.removeEventListener('beforeunload', handleBeforeUnload);
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'p') {
        e.preventDefault();
        if (currentProject) setShowQuickOpen(true);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [currentProject]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'F') {
        e.preventDefault();
        if (currentProject) setShowGlobalSearch(true);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [currentProject]);

  useEffect(() => {
    const handler = () => {
      if (currentProject) setShowGlobalSearch(true);
    };
    window.addEventListener('open-global-search', handler);
    return () => window.removeEventListener('open-global-search', handler);
  }, [currentProject]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      autoSelectLock.current = true;
      await new Promise(r => setTimeout(r, 500));
      if (cancelled) return;
      const chip = await safeInvoke<string>('ai_get_target_chip');
      if (!cancelled && chip) setSelectedChip(chip);
      const port = await safeInvoke<string>('ai_get_flash_port');
      if (!cancelled && port) setSelectedPort(port);
      autoSelectLock.current = false;
    })();
    return () => { cancelled = true; autoSelectLock.current = false; };
  }, []);

  useEffect(() => {
    if (selectedChip) {
      safeInvoke('ai_set_target_chip', { chip: selectedChip });
      if (currentProject?.path) {
        safeInvoke('save_project_config', {
          projectPath: currentProject.path,
          chip: selectedChip,
          target: selectedChip,
          flashPort: selectedPort || null,
        });
      }
    }
  }, [selectedChip]);

  useEffect(() => {
    if (selectedPort) {
      safeInvoke('ai_set_flash_port', { port: selectedPort });
      if (currentProject?.path) {
        safeInvoke('save_project_config', {
          projectPath: currentProject.path,
          chip: selectedChip || null,
          target: selectedChip || null,
          flashPort: selectedPort,
        });
      }
    }
  }, [selectedPort]);

  useEffect(() => {
    if (currentProject) {
      loadConfig(currentProject.path);
      restoreCurrentCache();
      (async () => {
        autoSelectLock.current = true;
        try {
          const config = await safeInvoke<{ chip: string; target?: string; flash_port?: string }>(
            'load_project_config',
            { projectPath: currentProject.path }
          );
          if (config) {
            if (config.chip) setSelectedChip(config.chip);
            if (config.flash_port) setSelectedPort(config.flash_port);
          }
        } finally {
          autoSelectLock.current = false;
        }
      })();
    }
  }, [currentProject, loadConfig, restoreCurrentCache]);

  useEffect(() => {
    detectIDF();
  }, [detectIDF]);

  const getIdfPath = useCallback(() => {
    return idfStatus.active?.idf_path || settings.idfPath || '';
  }, [idfStatus.active?.idf_path, settings.idfPath]);

  const refreshPorts = useCallback(async () => {
    try {
      const idfPath = getIdfPath();
      const cmd = idfPath ? 'list_ports_with_idf' : 'list_ports';
      const args = idfPath ? { idfPath } : undefined;
      const ports = await safeInvoke<SerialPortInfo[]>(cmd, args);
      if (ports) {
        setAvailablePorts(ports);
      }
    } catch {}
  }, [getIdfPath]);

  useEffect(() => {
    refreshPorts();
  }, [refreshPorts]);

  useEffect(() => {
    for (const p of availablePorts) {
      const key = p.port_name || p.name || p.path;
      if (p.chip_type) {
        detectedChipsRef.current[key] = p.chip_type;
      }
    }
  }, [availablePorts]);

  useEffect(() => {
    if (!selectedPort || selectedChipRef.current) return;
    const p = availablePorts.find(x => (x.port_name || x.name || x.path) === selectedPort && x.chip_type);
    if (p?.chip_type) {
      console.log('[AutoSelect] Chip auto-selected:', p.chip_type, 'for port:', selectedPort);
      setSelectedChip(p.chip_type);
    }
  }, [selectedPort, availablePorts]);

  useEffect(() => {
    if (!selectedPort || selectedChipRef.current) return;
    const alreadyHas = availablePorts.some(x => (x.port_name || x.name || x.path) === selectedPort && x.chip_type);
    if (alreadyHas) return;

    let cancelled = false;
    (async () => {
      try {
        const idfPath = getIdfPath();
        if (!idfPath) return;
        const ports = await safeInvoke<SerialPortInfo[]>('list_ports_with_idf', { idfPath });
        if (cancelled || !ports) return;
        setAvailablePorts(ports);
        for (const pp of ports) {
          if (pp.chip_type) detectedChipsRef.current[pp.port_name || pp.name || pp.path] = pp.chip_type;
        }
        const target = ports.find(x => (x.port_name || x.name || x.path) === selectedPort && x.chip_type);
        if (target?.chip_type && !selectedChipRef.current && !cancelled) {
          console.log('[AutoSelect] Chip auto-selected via esptool:', target.chip_type);
          setSelectedChip(target.chip_type);
        }
      } catch {}
    })();
    return () => { cancelled = true; };
  }, [selectedPort, getIdfPath]);

  const prevPortNamesRef = useRef<string[]>([]);
  const detectedChipsRef = useRef<Record<string, string>>({});
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const ports = await safeInvoke<SerialPortInfo[]>('list_ports');
        if (!ports) return;
        const currentNames = ports.map(p => p.port_name || p.name || p.path);
        const prevNames = prevPortNamesRef.current;
        const hasChanged = currentNames.some(n => !prevNames.includes(n))
                        || prevNames.some(n => !currentNames.includes(n))
                        || prevNames.length !== currentNames.length;

        if (hasChanged) {
          const newPorts = currentNames.filter(n => !prevNames.includes(n));

          for (const n of prevNames) {
            if (!currentNames.includes(n)) delete detectedChipsRef.current[n];
          }

          if (newPorts.length > 0) {
            const currentSelected = selectedPortRef.current;
            if (!currentSelected || !currentNames.includes(currentSelected)) {
              console.log('[AutoSelect] New port detected, auto-selecting:', newPorts[0]);
              setSelectedPort(newPorts[0]);
            }
          }

          refreshPorts();
          detectConnection(selectedPortRef.current || undefined);
        } else {
          const merged = ports.map(p => {
            const key = p.port_name || p.name || p.path;
            return { ...p, chip_type: p.chip_type || detectedChipsRef.current[key] };
          });
          setAvailablePorts(merged);
        }
        prevPortNamesRef.current = currentNames;
      } catch {}
    }, 2000);

    return () => clearInterval(interval);
  }, [refreshPorts, detectConnection]);

  useEffect(() => {
    if (selectedPort) {
      detectConnection(selectedPort);
    }
  }, [selectedPort, detectConnection]);

  useEffect(() => {
    const handler = (e: Event) => {
      const info = (e as CustomEvent<ConnectionInfo>).detail;
      if (!info || info.mode === 'unknown') return;

      if (info.port && !selectedPortRef.current) {
        console.log('[AutoSelect] Port auto-selected from connectionInfo:', info.port);
        setSelectedPort(info.port);
      }

      if (!selectedChipRef.current) {
        const chip = info.idfTarget
          || (info.chipHint && info.chipHint !== 'ESP32-USB-JTAG'
            ? info.chipHint : null);
        if (chip) {
          console.log('[AutoSelect] Chip auto-selected from VID/PID:', chip);
          setSelectedChip(chip);
        }
      }
    };

    window.addEventListener('esp-device-detected', handler);
    return () => window.removeEventListener('esp-device-detected', handler);
  }, []);

  const {
    buildOutput, setBuildOutput,
    isBuilding, isFlashing,
    isCleaning, isAnalyzingSize, isErasingFlash, isRunningBfm, isRunningDoctor,
    lastBuildSuccess,
    handleBuild, handleFlash, handleMonitor, handleMenuconfig: _handleMenuconfig,
    handleClean, handleSize, handleEraseFlash, handleBuildFlashMonitor, handleDoctor,
  } = useBuildOutput({
    currentProject,
    getIdfPath,
    selectedPort,
    openFile,
    bottomCollapsed,
    bottomPanelRef,
    setActiveBottomTab,
    refreshPorts,
  });

  const handleMenuconfig = useCallback(() => {
    console.log('[App] handleMenuconfig called, currentProject:', currentProject?.path);
    if (!currentProject) {
      showToast('error', t('toast.noProjectOpen'));
      return;
    }
    console.log('[App] setting showSdkConfig = true');
    showToast('info', 'Opening SDK Configuration...');
    setShowSdkConfig(true);
  }, [currentProject, t]);

  useEffect(() => {
    console.log('[App] showSdkConfig changed to:', showSdkConfig, 'currentProject:', currentProject?.path);
  }, [showSdkConfig, currentProject]);

  const {
    serialOutput, setSerialOutput, serialInput, setSerialInput,
    serialConnected, serialBaudRate, setSerialBaudRate,
    handleSerialConnect, handleSerialSend,
  } = useSerialMonitor({ selectedPort });

  const toggleBottom = () => {
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    else bottomPanelRef.current?.collapse();
  };

  const toggleAutoHardware = () => {
    if (autoHardwareCollapsed) autoHardwarePanelRef.current?.expand();
    else autoHardwarePanelRef.current?.collapse();
  };

  const handleOpenProject = useCallback(async () => {
    if (isTauri()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ directory: true, multiple: false, title: 'Open ESP32 Project' });
        if (selected && typeof selected === 'string') {
          await openProject(selected);
        }
        return;
      } catch (err) {
        console.error('Tauri open dialog failed:', err);
      }
    }

    try {
      if ('showDirectoryPicker' in window) {
        const dirHandle = await (window as any).showDirectoryPicker();
        const path = dirHandle.name;
        const virtualPath = `C:\\Projects\\${path}`;
        await openProject(virtualPath);
      } else {
        const path = prompt('Enter project path:', 'C:\\Projects\\ESP32');
        if (path) await openProject(path);
      }
    } catch (err: any) {
      if (err?.name !== 'AbortError') {
        const path = prompt('Enter project path:', 'C:\\Projects\\ESP32');
        if (path) await openProject(path);
      }
    }
  }, [openProject]);

  const handleNewFile = useCallback(() => {
    if (!currentProject) {
      showToast('error', t('toast.noProjectOpen'));
      return;
    }
    setDialogType('newFile');
  }, [currentProject, t]);

  const handleDialogConfirm = useCallback(async (value: string) => {
    setDialogType(null);
    if (!currentProject) return;

    if (dialogType === 'newFile') {
      try {
        await safeInvoke('create_file', { parentPath: currentProject.path, name: value, content: '' });
        const { loadDirectory } = useFileStore.getState();
        await loadDirectory(currentProject.path);
        showToast('success', t('toast.fileCreated', { name: value }));
      } catch (err) {
        showToast('error', t('toast.fileCreateFailed', { error: String(err) }));
      }
    } else if (dialogType === 'newFolder') {
      try {
        await safeInvoke('create_folder', { parentPath: currentProject.path, name: value });
        const { loadDirectory } = useFileStore.getState();
        await loadDirectory(currentProject.path);
        showToast('success', t('toast.folderCreated', { name: value }));
      } catch (err) {
        showToast('error', t('toast.folderCreateFailed', { error: String(err) }));
      }
    }
  }, [dialogType, currentProject]);

  const handleDialogCancel = useCallback(() => {
    setDialogType(null);
  }, []);

  const handleSave = useCallback(() => {
    if (activeTabId) saveFile(activeTabId);
  }, [activeTabId, saveFile]);

  if (!currentProject) {
    return (
      <>
        <WelcomeScreen
          onNewProject={() => setShowNewProject(true)}
          onOpenFolder={handleOpenProject}
        />
        {showNewProject && (
          <NewProjectDialog
            onClose={() => setShowNewProject(false)}
            onProjectCreated={(projectPath, chip) => {
              setViewMode('auto');
              try { localStorage.setItem('espsmith:viewMode', 'auto'); } catch {}
              if (chip) {
                setSelectedChip(chip);
                safeInvoke('ai_notify_chip_changed');
              }
              openProject(projectPath);
            }}
            onOpenSettings={() => setShowSettings(true)}
          />
        )}
        {showSettings && <SettingsDialog onClose={() => setShowSettings(false)} />}
      </>
    );
  }

  return (
    <div
      data-theme={viewMode}
      className="h-screen w-screen bg-surface-root text-text-primary overflow-hidden flex flex-col font-sans"
      onContextMenu={(e) => e.preventDefault()}
    >
      <header data-tauri-drag-region className="h-11 border-b border-border-default bg-surface-elevated shrink-0 flex items-center px-3 select-none">
        <LogoToggle mode={viewMode} onChange={handleViewModeChange} />

        <div className="w-px h-5 bg-border-default mx-3" />

        <div className="flex items-center gap-0.5">
          <ToolbarButton icon={FolderPlus} label={t('toolbar.newProject')} onClick={() => setShowNewProject(true)} />
          <ToolbarButton icon={FolderOpen} label={t('toolbar.openFolder')} onClick={handleOpenProject} />
          <ToolbarButton icon={Save} label={t('toolbar.save')} onClick={handleSave} />
        </div>

        {!isAutoMode && (
          <>
            <div className="w-px h-5 bg-border-default mx-1.5" />

            <div className="flex items-center gap-0.5">
              <ToolbarButton icon={Hammer} label={t('toolbar.build')} onClick={handleBuild} loading={isBuilding} />
              <ToolbarButton icon={Zap} label={t('toolbar.flash')} onClick={handleFlash} loading={isFlashing} />
              <ToolbarButton icon={Radio} label={t('toolbar.monitor')} onClick={handleMonitor} />
            </div>

            <div className="w-px h-5 bg-border-default mx-1.5" />

            <div className="flex items-center gap-0.5">
              <ToolbarButton icon={FilePlus} label={t('toolbar.newFile')} onClick={handleNewFile} />
              <ToolbarButton icon={SlidersHorizontal} label={t('toolbar.menuconfig')} onClick={handleMenuconfig} />
              <ToolbarButton icon={Trash2} label={t('toolbar.clean')} onClick={handleClean} loading={isCleaning} />
              <ToolbarButton icon={BarChart3} label={t('toolbar.size')} onClick={handleSize} loading={isAnalyzingSize} />
              <ToolbarButton icon={Eraser} label={t('toolbar.eraseFlash')} onClick={handleEraseFlash} loading={isErasingFlash} />
              <ToolbarButton icon={Play} label={t('toolbar.buildFlashMonitor')} onClick={handleBuildFlashMonitor} loading={isRunningBfm} />
              <ToolbarButton icon={HeartPulse} label="Doctor" onClick={handleDoctor} loading={isRunningDoctor} />
            </div>

            <div className="w-px h-5 bg-border-default mx-1.5" />
          </>
        )}

        <div className="flex-1" />

        {currentProject && (
          <div className="flex items-center gap-2 mr-3">
            <span className="text-[12px] text-text-secondary font-medium max-w-[160px] truncate">
              {currentProject.name}
            </span>
          </div>
        )}

        <div className="flex items-center gap-1.5 mr-2">
          <div className="flex items-center gap-1.5 h-[28px] w-[130px] px-2 bg-surface-overlay border border-border-subtle hover:border-border-default rounded-lg transition-all">
            <Cpu size={12} className="text-text-tertiary shrink-0" />
            <select
              value={selectedChip}
              onChange={(e) => setSelectedChip(e.target.value)}
              className="h-full flex-1 bg-transparent text-[11px] text-text-primary focus:outline-none cursor-pointer appearance-none"
              title="Target chip"
            >
              <option value="">-- Chip --</option>
              {(chipTargets.length > 0 ? chipTargets : [
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
              ]).map((chip) => (
                <option key={chip.target} value={chip.target}>{chip.label}{chip.is_preview ? ' (preview)' : ''}</option>
              ))}
            </select>
          </div>
          <div className="flex items-center gap-1.5 h-[28px] w-[130px] px-2 bg-surface-overlay border border-border-subtle rounded-lg hover:border-border-default transition-all">
            <Usb size={12} className="text-text-tertiary shrink-0" />
            <select
              value={selectedPort}
              onChange={(e) => setSelectedPort(e.target.value)}
              className="h-full flex-1 bg-transparent text-[11px] text-text-primary focus:outline-none cursor-pointer appearance-none"
              title="Flash port"
              onClick={() => refreshPorts()}
            >
              <option value="">-- Port --</option>
              {availablePorts.map((p) => {
                const portKey = p.port_name || p.path;
                const chipFromConnection = connectionInfo.chipHint && connectionInfo.port === portKey ? connectionInfo.chipHint : null;
                const chipLabel = p.chip_type || chipFromConnection;
                return (
                  <option key={portKey} value={portKey}>
                    {p.port_name || p.name}{chipLabel ? ` (${chipLabel})` : ''}
                  </option>
                );
              })}
            </select>
          </div>
          <ConnectionModeIndicator />
        </div>

        <div className="flex items-center gap-0.5 mr-1">
          <ToolbarButton icon={Settings} label={t('toolbar.settings')} onClick={() => setShowSettings(true)} />
        </div>

        {isAutoMode && (
          <>
            <div className="w-px h-5 bg-border-default mx-1.5" />
            <button
              onClick={toggleAutoHardware}
              className={`flex items-center gap-1.5 px-2 py-1.5 rounded-sm transition-all duration-150 ${
                autoHardwareCollapsed
                  ? 'bg-accent-muted text-accent'
                  : 'text-text-secondary hover:text-text-primary hover:bg-surface-hover'
              }`}
              title={autoHardwareCollapsed ? 'Show hardware panel' : 'Hide hardware panel'}
            >
              <PanelLeft size={15} className={autoHardwareCollapsed ? 'text-accent' : 'text-text-tertiary'} />
            </button>
          </>
        )}

        {isTauri() && (
          <div className="flex items-center -mr-3">
            <WindowControlButton onClick={handleMinimize} title={t('window.minimize')}>
              <Minus size={13} />
            </WindowControlButton>
            <WindowControlButton onClick={handleMaximize} title={isWindowMaximized ? t('window.restore') : t('window.maximize')}>
              {isWindowMaximized ? <Copy size={12} /> : <Square size={11} />}
            </WindowControlButton>
            <WindowControlButton onClick={handleClose} title={t('window.close')} isClose>
              <X size={14} />
            </WindowControlButton>
          </div>
        )}
      </header>

      {updateBanner && (
        <div className={`flex items-center justify-between px-4 py-2 bg-accent/90 text-white text-[13px] font-medium shrink-0 ${updateBannerDownloading ? 'cursor-wait' : ''}`}>
          <span>{t('settings.updateBanner', { version: updateBanner.version })}</span>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setUpdateBanner(null)}
              disabled={updateBannerDownloading}
              className="px-3 py-1 text-[12px] bg-white/20 rounded-md hover:bg-white/30 transition-colors disabled:opacity-50"
            >
              {t('settings.updateBannerDismiss')}
            </button>
            <button
              onClick={async () => {
                try {
                  setUpdateBannerDownloading(true);
                  const { check } = await import('@tauri-apps/plugin-updater');
                  const { relaunch } = await import('@tauri-apps/plugin-process');
                  const update = await check();
                  if (update) {
                    await update.downloadAndInstall();
                    setTimeout(() => relaunch(), 500);
                  }
                } catch {}
                setUpdateBannerDownloading(false);
              }}
              disabled={updateBannerDownloading}
              className="flex items-center gap-1 px-3 py-1 text-[12px] bg-white rounded-md text-accent font-semibold hover:bg-white/90 transition-colors disabled:opacity-50"
            >
              {updateBannerDownloading ? <Loader2 size={12} className="animate-spin" /> : <Download size={12} />}
              {t('settings.updateBannerInstall')}
            </button>
          </div>
        </div>
      )}

      {isAutoMode ? (
        <PanelGroup key="auto" direction="horizontal" className="flex-1 min-h-0">
          <Panel
            ref={autoHardwarePanelRef}
            defaultSize={28}
            minSize={20}
            maxSize={45}
            collapsible
            collapsedSize={0}
            onCollapse={() => setAutoHardwareCollapsed(true)}
            onExpand={() => setAutoHardwareCollapsed(false)}
            className="bg-surface-base"
          >
            <div className="h-full border-r border-border-default">
              <HardwareStore />
            </div>
          </Panel>

          <PanelResizeHandle className="w-1 bg-border-default hover:bg-accent/40 transition-colors data-[resize-handle-active]:bg-accent" />

          <Panel defaultSize={72} minSize={35} className="bg-surface-base">
            <div className="h-full border-l border-border-default">
              <ChatPanel />
            </div>
          </Panel>
        </PanelGroup>
      ) : (
        <PanelGroup key="code" direction="horizontal" className="flex-1 min-h-0">
          <Panel
            ref={leftPanelRef}
            defaultSize={19}
            minSize={12}
            maxSize={35}
            collapsible
            collapsedSize={0}
            onCollapse={() => {}}
            onExpand={() => {}}
            className="bg-surface-base"
          >
            <div className="h-full flex flex-col border-r border-border-default">
              <div className="flex border-b border-border-default">
                <PanelTab
                  active={leftPanel === 'files'}
                  icon={FolderOpen}
                  label={t('leftPanel.explorer')}
                  onClick={() => setLeftPanel('files')}
                />
                <PanelTab
                  active={leftPanel === 'hardware'}
                  icon={Cpu}
                  label={t('leftPanel.hardware')}
                  onClick={() => setLeftPanel('hardware')}
                />
              </div>

              <PanelGroup direction="vertical" className="flex-1">
                <Panel defaultSize={62} minSize={30}>
                  <div className="h-full overflow-hidden">
                    {leftPanel === 'files' ? <FileTree /> : <HardwareStore />}
                  </div>
                </Panel>
                <PanelResizeHandle className="h-1 bg-border-default hover:bg-accent/40 transition-colors data-[resize-handle-active]:bg-accent" />
                <Panel defaultSize={38} minSize={15}>
                  <div className="h-full">
                    <GitPanel />
                  </div>
                </Panel>
              </PanelGroup>
            </div>
          </Panel>

          <PanelResizeHandle className="w-1 bg-border-default hover:bg-accent/40 transition-colors data-[resize-handle-active]:bg-accent" />

          <Panel defaultSize={55} minSize={25}>
            <div className="h-full flex flex-col">
              <PanelGroup direction="vertical">
                <Panel defaultSize={70} minSize={30}>
                  <div className="h-full bg-surface-root">
                    <CodeEditor />
                  </div>
                </Panel>

                <PanelResizeHandle className="h-1 bg-border-default hover:bg-accent/40 transition-colors data-[resize-handle-active]:bg-accent" />

                <Panel
                  ref={bottomPanelRef}
                  defaultSize={30}
                  minSize={10}
                  maxSize={50}
                  collapsible
                  collapsedSize={0}
                  onCollapse={() => setBottomCollapsed(true)}
                  onExpand={() => setBottomCollapsed(false)}
                >
                  <div className="h-full flex flex-col border-t border-border-default bg-surface-elevated">
                    <div className="flex items-center justify-between border-b border-border-default px-2 shrink-0">
                      <div className="flex">
                        <BottomTabButton
                          active={activeBottomTab === 'build'}
                          icon={Hammer}
                          label={t('bottomPanel.buildOutput')}
                          onClick={() => setActiveBottomTab('build')}
                        />
                        <BottomTabButton
                          active={activeBottomTab === 'serial'}
                          icon={Terminal}
                          label={t('bottomPanel.serialMonitor')}
                          onClick={() => setActiveBottomTab('serial')}
                        />
                        <BottomTabButton
                          active={activeBottomTab === 'debug'}
                          icon={Bug}
                          label={t('bottomPanel.debugConsole')}
                          onClick={() => setActiveBottomTab('debug')}
                        />
                      </div>
                      <button
                        className="p-1.5 text-text-tertiary hover:text-text-primary hover:bg-surface-hover rounded-sm transition-colors"
                        onClick={toggleBottom}
                      >
                        <ChevronDown size={14} />
                      </button>
                    </div>
                    <div className="flex-1 overflow-hidden">
                      {activeBottomTab === 'build' ? (
                        <BuildOutputPanel
                          output={buildOutput}
                          onClear={() => setBuildOutput([])}
                          isBuilding={isBuilding || isFlashing}
                        />
                      ) : activeBottomTab === 'serial' ? (
                        <SerialMonitorPanel
                          output={serialOutput}
                          input={serialInput}
                          connected={serialConnected}
                          port={selectedPort}
                          baudRate={serialBaudRate}
                          onInputChange={setSerialInput}
                          onSend={handleSerialSend}
                          onConnect={handleSerialConnect}
                          onBaudRateChange={setSerialBaudRate}
                          onClear={() => setSerialOutput([])}
                        />
                      ) : (
                        <DebugPanel targetChip={selectedChip} />
                      )}
                    </div>
                  </div>
                </Panel>
              </PanelGroup>
            </div>
          </Panel>

          <PanelResizeHandle className="w-1 bg-border-default hover:bg-accent/40 transition-colors data-[resize-handle-active]:bg-accent" />

          <Panel
            ref={rightPanelRef}
            defaultSize={26}
            minSize={15}
            maxSize={45}
            collapsible
            collapsedSize={0}
            onCollapse={() => {}}
            onExpand={() => {}}
            className="bg-surface-base"
          >
            <div className="h-full border-l border-border-default">
              <ChatPanel />
            </div>
          </Panel>
        </PanelGroup>
      )}

      <footer className="h-6 border-t border-border-default flex items-center px-3 text-[11px] bg-surface-elevated text-text-secondary shrink-0 select-none">
        <div className="flex items-center gap-1.5">
          <span className="flex items-center gap-1 px-1 py-0.5 rounded-sm hover:bg-surface-hover hover:text-text-primary transition-colors cursor-pointer">
            <GitBranch size={11} />
            <span>main</span>
          </span>
          <span className="w-px h-3 bg-border-subtle" />
          <span className="flex items-center gap-1 px-1 py-0.5">
            <span className={`w-1.5 h-1.5 rounded-full ${isBuilding || isFlashing ? 'animate-pulse-dot bg-warning' : lastBuildSuccess === false ? 'bg-red-400' : 'bg-green-400'}`} />
            <span>{isBuilding ? t('statusBar.building') : isFlashing ? t('statusBar.flashing') : lastBuildSuccess === false ? t('statusBar.buildFailed') : lastBuildSuccess === true ? t('statusBar.buildSucceeded') : t('statusBar.ready')}</span>
          </span>
          {currentProject && (
            <>
              <span className="w-px h-3 bg-border-subtle" />
              <span className="px-1 py-0.5">{currentProject.chip}</span>
            </>
          )}
        </div>
        <div className="flex-1" />
        <div className="flex items-center gap-1.5">
          {connectionMode !== 'unknown' && connectionInfo.port && (
            <>
              <span className="flex items-center gap-1 px-1 py-0.5">
                {connectionMode === 'jtag'
                  ? <Usb size={10} className="text-amber-400" />
                  : <Radio size={10} className="text-emerald-400" />
                }
                <span>{connectionMode === 'jtag' ? t('hardware.connectionMode.jtag') : t('hardware.connectionMode.uart')}</span>
              </span>
              <span className="w-px h-3 bg-border-subtle" />
            </>
          )}
          {editorLanguage && (
            <>
              <span className="px-1 py-0.5">{editorLanguage === 'c' ? 'C' : editorLanguage === 'cpp' ? 'C++' : editorLanguage.charAt(0).toUpperCase() + editorLanguage.slice(1)}</span>
              <span className="w-px h-3 bg-border-subtle" />
            </>
          )}
          {activeTabId && (
            <>
              <span className="px-1 py-0.5 tabular-nums">Ln {cursorLine}, Col {cursorColumn}</span>
              <span className="w-px h-3 bg-border-subtle" />
            </>
          )}
          <span className="px-1 py-0.5">Spaces: 4</span>
          <span className="w-px h-3 bg-border-subtle" />
          <span className="px-1 py-0.5">UTF-8</span>
          {currentProject && (
            <>
              <span className="w-px h-3 bg-border-subtle" />
              <span className="px-1 py-0.5">ESP-IDF {currentProject.idf_version}</span>
            </>
          )}
        </div>
      </footer>

      <InputDialog
        open={showInputDialog}
        title={dialogType === 'newFile' ? t('dialog.newFile') : t('dialog.newFolder')}
        placeholder={dialogType === 'newFile' ? 'untitled.c' : 'components'}
        label={dialogType === 'newFile' ? t('dialog.fileName') : t('dialog.folderName')}
        cancelLabel={t('dialog.cancel')}
        okLabel={t('dialog.ok')}
        onConfirm={handleDialogConfirm}
        onCancel={handleDialogCancel}
      />
      {showNewProject && (
        <NewProjectDialog
          onClose={() => setShowNewProject(false)}
          onProjectCreated={(projectPath, chip) => {
            setViewMode('auto');
            try { localStorage.setItem('espsmith:viewMode', 'auto'); } catch {}
            if (chip) {
              setSelectedChip(chip);
              safeInvoke('ai_notify_chip_changed');
            }
            openProject(projectPath);
          }}
          onOpenSettings={() => setShowSettings(true)}
        />
      )}
      {showSettings && <SettingsDialog onClose={() => setShowSettings(false)} />}
      {showGlobalSearch && <GlobalSearchPanel onClose={() => setShowGlobalSearch(false)} />}
      {showQuickOpen && <QuickOpenDialog onClose={() => setShowQuickOpen(false)} />}
      {showSdkConfig && currentProject && (
        <div className="fixed inset-0 z-50 bg-surface-base">
          <SdkConfigEditor
            projectPath={currentProject.path}
            idfPath={getIdfPath()}
            onClose={() => setShowSdkConfig(false)}
          />
        </div>
      )}

      <ToastContainer />
    </div>
  );
}

function ToolbarButton({
  icon: Icon,
  label,
  onClick,
  loading,
}: {
  icon: React.ComponentType<{ size?: number | string; className?: string }>;
  label: string;
  onClick?: () => void;
  loading?: boolean;
}) {
  return (
    <button
      className="flex items-center gap-1.5 px-2 py-1.5 text-text-secondary hover:text-text-primary hover:bg-surface-hover rounded-sm transition-all duration-150 group"
      title={label}
      onClick={onClick}
      disabled={loading}
    >
      {loading ? (
        <Loader2 size={15} className="animate-spin text-accent" />
      ) : (
        <Icon size={15} className="text-text-tertiary group-hover:text-text-primary transition-colors" />
      )}
    </button>
  );
}

function PanelTab({
  active,
  icon: Icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: React.ComponentType<{ size?: number | string; className?: string }>;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex-1 flex items-center justify-center gap-1.5 py-2 text-[11px] font-medium tracking-wide uppercase transition-all duration-150 border-b-2 ${
        active
          ? 'border-accent text-text-primary bg-surface-elevated'
          : 'border-transparent text-text-tertiary hover:text-text-secondary hover:bg-surface-hover'
      }`}
    >
      <Icon size={13} />
      {label}
    </button>
  );
}

function BottomTabButton({
  active,
  icon: Icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: React.ComponentType<{ size?: number | string; className?: string }>;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-1.5 px-3 py-2 text-[12px] transition-all duration-150 border-b-2 ${
        active
          ? 'border-accent text-text-primary'
          : 'border-transparent text-text-tertiary hover:text-text-secondary'
      }`}
    >
      <Icon size={13} />
      {label}
    </button>
  );
}

function WindowControlButton({
  children,
  onClick,
  title,
  isClose,
}: {
  children: React.ReactNode;
  onClick: () => void;
  title: string;
  isClose?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className={`flex items-center justify-center w-11 h-11 transition-colors ${
        isClose
          ? 'text-text-tertiary hover:text-white hover:bg-red-500'
          : 'text-text-tertiary hover:text-text-primary hover:bg-surface-hover'
      }`}
    >
      {children}
    </button>
  );
}

function LogoToggle({
  mode,
  onChange,
}: {
  mode: ViewMode;
  onChange: (m: ViewMode) => void;
}) {
  const isCode = mode === 'code';

  return (
    <button
      onClick={() => onChange(isCode ? 'auto' : 'code')}
      className="relative flex items-center h-8 w-[70px] rounded-md bg-surface-base border border-border-subtle overflow-hidden cursor-pointer transition-colors hover:border-border-default"
      title={isCode ? 'Switch to Auto mode' : 'Switch to Code mode'}
    >
      <div
        className={`absolute top-0.5 bottom-0.5 aspect-square rounded bg-surface-elevated shadow-sm flex items-center justify-center transition-all duration-300 ease-in-out ${
          isCode ? 'right-0.5' : 'left-0.5'
        }`}
      >
        <img src={isCode ? '/logo-b.png' : '/logo-w.png'} alt="" className="h-5 w-auto object-contain" />
      </div>

      <div className={`relative z-10 flex items-center justify-center flex-1 h-full transition-opacity duration-200 ${isCode ? 'opacity-60' : 'opacity-0'}`}>
        <span className="text-[11px] font-semibold tracking-wide text-text-primary ml-1">CODE</span>
      </div>

      <div className={`relative z-10 flex items-center justify-center flex-1 h-full transition-opacity duration-200 ${isCode ? 'opacity-0' : 'opacity-60'}`}>
        <span className="text-[11px] font-semibold tracking-wide text-text-primary mr-1">AUTO</span>
      </div>
    </button>
  );
}

export default App;

function ConnectionModeIndicator() {
  const { t } = useTranslation();
  const { connectionMode, detectConnection } = useHardwareStore();

  useEffect(() => {
    detectConnection();
  }, [detectConnection]);

  if (connectionMode === 'unknown') {
    return (
      <span
        className="px-2 py-0.5 text-[10px] font-medium bg-surface-hover text-text-tertiary rounded-sm border border-border-subtle cursor-pointer hover:bg-surface-active"
        onClick={() => detectConnection()}
        title={t('hardware.connectionMode.unknownDesc')}
      >
        <Usb size={10} className="inline mr-0.5" />
        {t('hardware.connectionMode.unknown')}
      </span>
    );
  }

  if (connectionMode === 'jtag') {
    return (
      <span
        className="px-2 py-0.5 text-[10px] font-semibold text-white bg-gradient-to-r from-amber-500 to-orange-500 rounded-sm border border-amber-400/50 shadow-sm shadow-amber-500/20 cursor-pointer"
        onClick={() => detectConnection()}
        title={t('hardware.connectionMode.jtagDesc')}
      >
        <Usb size={10} className="inline mr-0.5" />
        {t('hardware.connectionMode.jtag')}
      </span>
    );
  }

  return (
    <span
      className="px-2 py-0.5 text-[10px] font-semibold text-white bg-gradient-to-r from-emerald-500 to-green-500 rounded-sm border border-emerald-400/50 shadow-sm shadow-emerald-500/20 cursor-pointer"
      onClick={() => detectConnection()}
      title={`${t('hardware.connectionMode.uartDesc')}\n${t('hardware.connectionMode.switchToJtag')}`}
    >
      <Radio size={10} className="inline mr-0.5" />
      {t('hardware.connectionMode.uart')}
    </span>
  );
}
