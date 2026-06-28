import { useState, useRef, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { showToast } from '../components/ui/Toast';
import { safeInvoke } from '../lib/invoke';
import { listen } from '@tauri-apps/api/event';
import type { ImperativePanelHandle } from 'react-resizable-panels';

type BottomTab = 'build' | 'serial' | 'debug';

interface UseBuildOutputOptions {
  currentProject: { name: string; path: string } | null;
  getIdfPath: () => string | null;
  selectedPort: string;
  openFile: (path: string) => Promise<void>;
  bottomCollapsed: boolean;
  bottomPanelRef: React.RefObject<ImperativePanelHandle | null>;
  setActiveBottomTab: (tab: BottomTab) => void;
  refreshPorts: () => void;
}

export function useBuildOutput(options: UseBuildOutputOptions) {
  const { t } = useTranslation();
  const { currentProject, getIdfPath, selectedPort, openFile, bottomCollapsed, bottomPanelRef, setActiveBottomTab, refreshPorts } = options;

  const [buildOutput, setBuildOutput] = useState<string[]>([]);
  const [isBuilding, setIsBuilding] = useState(false);
  const [isFlashing, setIsFlashing] = useState(false);
  const [isCleaning, setIsCleaning] = useState(false);
  const [isAnalyzingSize, setIsAnalyzingSize] = useState(false);
  const [isErasingFlash, setIsErasingFlash] = useState(false);
  const [isRunningBfm, setIsRunningBfm] = useState(false);
  const [isRunningDoctor, setIsRunningDoctor] = useState(false);
  const [lastBuildSuccess, setLastBuildSuccess] = useState<boolean | null>(null);
  const buildBufferRef = useRef<string[]>([]);
  const aiOpTypeRef = useRef<string | null>(null);

  useEffect(() => {
    const interval = setInterval(() => {
      if (buildBufferRef.current.length > 0) {
        const batch = buildBufferRef.current;
        buildBufferRef.current = [];
        setBuildOutput((prev) => [...prev, ...batch].slice(-1000));
      }
    }, 100);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    let unlistenOutput: (() => void) | undefined;
    let unlistenDone: (() => void) | undefined;
    let unlistenOpProgress: (() => void) | undefined;
    let unlistenOpDone: (() => void) | undefined;
    let cancelled = false;

    listen<{ line: string; is_stderr: boolean }>('build-output', (event) => {
      buildBufferRef.current.push(event.payload.line);
    }).then((unlisten) => { if (!cancelled) { unlistenOutput = unlisten; } else { unlisten(); } });

    listen<{ success: boolean }>('build-done', (event) => {
      if (event.payload.success) {
        buildBufferRef.current.push('\n✅ Build finished successfully.\n');
      } else {
        buildBufferRef.current.push('\n❌ Build failed.\n');
      }
      setLastBuildSuccess(event.payload.success);
      setIsBuilding(false);
      setIsFlashing(false);
    }).then((unlisten) => { if (!cancelled) { unlistenDone = unlisten; } else { unlisten(); } });

    // AI 触发的编译/烧录操作同步 isBuilding/isFlashing，让状态栏和 BuildOutputPanel 动画生效
    listen<{ operationType: string }>('ai-operation-progress', (event) => {
      const opType = event.payload.operationType;
      aiOpTypeRef.current = opType;
      if (opType === 'build') {
        setIsBuilding(true);
      } else if (opType === 'flash') {
        setIsFlashing(true);
      }
    }).then((unlisten) => { if (!cancelled) { unlistenOpProgress = unlisten; } else { unlisten(); } });

    listen('ai-operation-done', () => {
      const opType = aiOpTypeRef.current;
      if (opType === 'build') {
        setIsBuilding(false);
      } else if (opType === 'flash') {
        setIsFlashing(false);
      }
      aiOpTypeRef.current = null;
    }).then((unlisten) => { if (!cancelled) { unlistenOpDone = unlisten; } else { unlisten(); } });

    return () => {
      cancelled = true;
      unlistenOutput?.();
      unlistenDone?.();
      unlistenOpProgress?.();
      unlistenOpDone?.();
    };
  }, []);

  async function promptForPort(): Promise<string | null> {
    try {
      const ports = await safeInvoke<Array<{ path: string }>>('list_ports');
      if (ports && ports.length > 0) return ports[0].path;
    } catch { /* ignore */ }
    // No browser prompt — return null so caller shows a toast asking the user to pick a port.
    return null;
  }

  const handleBuild = useCallback(async () => {
    if (!currentProject) { showToast('error', t('toast.noProjectOpen')); return; }
    const idfPath = getIdfPath();
    if (!idfPath) { showToast('error', t('toast.idfNotFound')); return; }
    setIsBuilding(true);
    setLastBuildSuccess(null);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    setBuildOutput([]);
    setBuildOutput((prev) => [...prev, `\n🔨 Building ${currentProject.name}...\n`]);
    safeInvoke('idf_build', { projectPath: currentProject.path, idfPath });
  }, [currentProject, getIdfPath, bottomCollapsed, t]);

  const handleFlash = useCallback(async () => {
    if (!currentProject) { showToast('error', t('toast.noProjectOpen')); return; }
    const idfPath = getIdfPath();
    if (!idfPath) { showToast('error', t('toast.idfNotFound')); return; }
    const port = selectedPort || (await promptForPort());
    if (!port) { showToast('warning', t('toast.selectSerialPortFirst')); return; }
    setIsFlashing(true);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    setBuildOutput([]);
    setBuildOutput((prev) => [...prev, `\n⚡ Flashing ${currentProject.name} to ${port}...\n`]);
    safeInvoke('idf_flash', { projectPath: currentProject.path, idfPath, port });
  }, [currentProject, getIdfPath, selectedPort, bottomCollapsed, t]);

  const handleMonitor = useCallback(() => {
    setActiveBottomTab('serial');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    refreshPorts();
  }, [bottomCollapsed, refreshPorts]);

  const handleMenuconfig = useCallback(async () => {
    if (!currentProject) { showToast('error', t('toast.noProjectOpen')); return; }
    const idfPath = getIdfPath();
    if (!idfPath) { showToast('error', t('toast.idfNotFound')); return; }
    setBuildOutput((prev) => [...prev, `\n⚙ Launching menuconfig for ${currentProject.name} (opens in external terminal)...\n`]);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    try {
      await safeInvoke('idf_menuconfig', { projectPath: currentProject.path, idfPath });
      setBuildOutput((prev) => [...prev, '✅ menuconfig launched in external terminal window.\n']);
    } catch (err) {
      setBuildOutput((prev) => [...prev, `⚠ menuconfig failed: ${err} — opening sdkconfig as text instead.\n`]);
      try {
        await openFile(`${currentProject.path}\\sdkconfig`);
      } catch {
        try {
          await openFile(`${currentProject.path}\\sdkconfig.defaults`);
        } catch {
          showToast('error', t('toast.sdkconfigNotFound'));
        }
      }
    }
  }, [currentProject, getIdfPath, bottomCollapsed, openFile, t]);

  const handleClean = useCallback(async () => {
    if (!currentProject) { showToast('error', t('toast.noProjectOpen')); return; }
    const idfPath = getIdfPath();
    if (!idfPath) { showToast('error', t('toast.idfNotFound')); return; }
    setIsCleaning(true);
    setBuildOutput((prev) => [...prev, `\n🧹 Cleaning ${currentProject.name}...\n`]);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    try {
      const result = await safeInvoke<string>('idf_clean', { projectPath: currentProject.path, idfPath });
      if (result) setBuildOutput((prev) => [...prev, result, '\n✅ Clean finished.\n']);
    } catch (err) {
      setBuildOutput((prev) => [...prev, `\n❌ Clean error: ${err}\n`]);
    } finally {
      setIsCleaning(false);
    }
  }, [currentProject, getIdfPath, bottomCollapsed]);

  const handleSize = useCallback(async () => {
    if (!currentProject) { showToast('error', t('toast.noProjectOpen')); return; }
    const idfPath = getIdfPath();
    if (!idfPath) { showToast('error', t('toast.idfNotFound')); return; }
    setIsAnalyzingSize(true);
    setBuildOutput((prev) => [...prev, `\n📊 Analyzing firmware size for ${currentProject.name}...\n`]);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    try {
      const result = await safeInvoke<{ components?: Array<{ name: string; size_bytes: number; size_kb: number }>; raw_output?: string }>('idf_size_json', { projectPath: currentProject.path, idfPath });
      if (result?.components && result.components.length > 0) {
        const maxSize = Math.max(...result.components.map(c => c.size_bytes), 1);
        const barWidth = 30;
        const lines: string[] = [];
        const totalBytes = result.components.reduce((sum, c) => sum + c.size_bytes, 0);
        lines.push('┌──────────────────────────────────────────────────────────────────┐');
        lines.push('│ Component                │    Size (KB) │ %Total │ Memory        │');
        lines.push('├──────────────────────────┼──────────────┼────────┼───────────────┤');
        for (const c of result.components) {
          const pct = ((c.size_bytes / totalBytes) * 100).toFixed(1);
          const barLen = Math.max(1, Math.round((c.size_bytes / maxSize) * barWidth));
          const bar = '█'.repeat(barLen);
          const name = c.name.padEnd(24).substring(0, 24);
          const kb = c.size_kb.toFixed(1).padStart(10);
          const pctStr = pct.padStart(5) + '%';
          lines.push(`│ ${name} │ ${kb} │ ${pctStr} │ ${bar.padEnd(barWidth)} │`);
        }
        lines.push('├──────────────────────────┼──────────────┼────────┼───────────────┤');
        lines.push(`│ TOTAL                    │ ${(totalBytes / 1024).toFixed(1).padStart(10)} │ 100.0% │ Flash: ${(totalBytes / 1024).toFixed(0)} KB │`);
        lines.push('└──────────────────────────────────────────────────────────────────┘');
        setBuildOutput((prev) => [...prev, ...lines, '']);
      } else {
        setBuildOutput((prev) => [...prev, result?.raw_output || '(no output)', '\n✅ Size analysis complete.\n']);
      }
    } catch (err) {
      setBuildOutput((prev) => [...prev, `\n❌ Size analysis error: ${err}\n`]);
    } finally {
      setIsAnalyzingSize(false);
    }
  }, [currentProject, getIdfPath, bottomCollapsed]);

  const handleEraseFlash = useCallback(async () => {
    if (!currentProject) { showToast('error', t('toast.noProjectOpen')); return; }
    const idfPath = getIdfPath();
    if (!idfPath) { showToast('error', t('toast.idfNotFound')); return; }
    const port = selectedPort || (await promptForPort());
    if (!port) { showToast('warning', t('toast.selectSerialPortFirst')); return; }
    setIsErasingFlash(true);
    setBuildOutput((prev) => [...prev, `\n🗑 Erasing flash on ${port}...\n`]);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    try {
      const result = await safeInvoke<string>('idf_erase_flash', { projectPath: currentProject.path, idfPath, port });
      if (result) setBuildOutput((prev) => [...prev, result, '\n✅ Flash erased.\n']);
    } catch (err) {
      setBuildOutput((prev) => [...prev, `\n❌ Erase error: ${err}\n`]);
    } finally {
      setIsErasingFlash(false);
    }
  }, [currentProject, getIdfPath, selectedPort, bottomCollapsed]);

  const handleBuildFlashMonitor = useCallback(async () => {
    if (!currentProject) { showToast('error', t('toast.noProjectOpen')); return; }
    const idfPath = getIdfPath();
    if (!idfPath) { showToast('error', t('toast.idfNotFound')); return; }
    const port = selectedPort || (await promptForPort());
    if (!port) { showToast('warning', t('toast.selectSerialPortFirst')); return; }
    setIsRunningBfm(true);
    setBuildOutput((prev) => [...prev, `\n🚀 Build + Flash + Monitor for ${currentProject.name} on ${port}...\nOpened in external terminal window.\n`]);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    try {
      await safeInvoke('idf_build_flash_monitor', { projectPath: currentProject.path, idfPath, port });
    } catch (err) {
      setBuildOutput((prev) => [...prev, `\n❌ Build+Flash+Monitor error: ${err}\n`]);
    } finally {
      setIsRunningBfm(false);
    }
  }, [currentProject, getIdfPath, selectedPort, bottomCollapsed]);

  const handleDoctor = useCallback(async () => {
    const idfPath = getIdfPath();
    setIsRunningDoctor(true);
    setBuildOutput(['\n' + '='.repeat(60) + '\n  ESP-IDF Environment Doctor\n' + '='.repeat(60) + '\n']);
    setActiveBottomTab('build');
    if (bottomCollapsed) bottomPanelRef.current?.expand();
    try {
      const result = await safeInvoke<{ health: string; pass: number; fail: number; total: number; checks: Array<{ name: string; status: string; detail: string }> }>(
        'idf_doctor', { projectPath: currentProject?.path || null, idfPath: idfPath || null }
      );
      if (result?.checks) {
        for (const check of result.checks) {
          const icon = check.status === 'ok' ? '✅' : check.status === 'error' ? '❌' : check.status === 'warn' ? '⚠' : 'ℹ';
          setBuildOutput((prev) => [...prev, `${icon} ${check.name}: ${check.detail}`]);
        }
        const healthEmoji = result.health === 'healthy' ? '🟢' : result.health === 'warn' ? '🟡' : '🔴';
        setBuildOutput((prev) => [...prev, '', `${healthEmoji} Health: ${result.health} | ${result.pass}/${result.total} passed, ${result.fail} failed`, '='.repeat(60) + '\n']);
      }
    } catch (err) {
      setBuildOutput((prev) => [...prev, `\n❌ Doctor check failed: ${err}\n`]);
    } finally {
      setIsRunningDoctor(false);
    }
  }, [getIdfPath, currentProject, bottomCollapsed]);

  return {
    buildOutput,
    setBuildOutput,
    buildBufferRef,
    isBuilding,
    setIsBuilding,
    isFlashing,
    setIsFlashing,
    isCleaning,
    isAnalyzingSize,
    isErasingFlash,
    isRunningBfm,
    isRunningDoctor,
    lastBuildSuccess,
    handleBuild,
    handleFlash,
    handleMonitor,
    handleMenuconfig,
    handleClean,
    handleSize,
    handleEraseFlash,
    handleBuildFlashMonitor,
    handleDoctor,
  };
}
