import { useState, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Play, Square, CornerDownRight, ArrowDown, ArrowUpRight,
  Trash2, Plus, X, RefreshCw,
} from 'lucide-react';
import { safeInvoke } from '../../lib/invoke';
import type { DebugState, Breakpoint, StackFrame, VariableInfo } from '../../types';

interface DebugPanelProps {
  targetChip?: string | null;
}

type SubTab = 'console' | 'breakpoints' | 'variables' | 'stack' | 'registers';

export function DebugPanel({ targetChip }: DebugPanelProps) {
  const { t } = useTranslation();
  const outputRef = useRef<HTMLDivElement>(null);

  const [connected, setConnected] = useState(false);
  const [output, setOutput] = useState<string[]>([]);
  const [debugState, setDebugState] = useState<DebugState | null>(null);
  const [breakpoints, setBreakpoints] = useState<Breakpoint[]>([]);
  const [watchedVars, setWatchedVars] = useState<VariableInfo[]>([]);
  const [watchInput, setWatchInput] = useState('');
  const [subTab, setSubTab] = useState<SubTab>('console');
  const [elfPath, setElfPath] = useState('');

  const addOutput = useCallback((...lines: string[]) => {
    setOutput((prev) => [...prev, ...lines]);
    setTimeout(() => outputRef.current?.scrollTo(0, outputRef.current.scrollHeight), 50);
  }, []);

  const refreshState = useCallback(async () => {
    if (!connected) return;
    try {
      const state = await safeInvoke<DebugState>('debug_get_state');
      if (state) setDebugState(state);
    } catch { /* ignore */ }
  }, [connected]);

  const refreshBreakpoints = useCallback(async () => {
    if (!connected) return;
    try {
      const bps = await safeInvoke<Breakpoint[]>('debug_list_breakpoints');
      if (bps) setBreakpoints(bps);
    } catch { /* ignore */ }
  }, [connected]);

  const handleAttach = useCallback(async () => {
    if (connected) {
      try { await safeInvoke('debug_stop'); } catch { /* ignore */ }
      setConnected(false);
      setDebugState(null);
      setBreakpoints([]);
      setWatchedVars([]);
      addOutput('\n--- GDB detached ---\n');
      return;
    }

    if (!elfPath.trim()) {
      addOutput(`\n${t('debug.enterElfPath')}\n`);
      return;
    }

    try {
      addOutput(`\n--- Connecting GDB (${targetChip || 'esp32'}) to localhost:3333 ---\n`);
      const state = await safeInvoke<DebugState>('debug_start', {
        elfPath: elfPath.trim(),
        target: 'localhost:3333',
        targetChip: targetChip || null,
      });
      if (state) {
        setConnected(true);
        setDebugState(state);
        addOutput(`Connected. PC = ${state.pc}`);
        state.stack.slice(0, 5).forEach((f: StackFrame) => {
          addOutput(`  #${f.level} ${f.function} at ${f.file}:${f.line}`);
        });
        await refreshBreakpoints();
      }
    } catch (err) {
      addOutput(`\n${t('debug.connectFailed', { error: String(err) })}\n`);
    }
  }, [connected, elfPath, targetChip, addOutput, refreshBreakpoints]);

  const handleContinue = useCallback(async () => {
    if (!connected) return;
    try {
      addOutput('\n▶ Continuing...');
      const state = await safeInvoke<DebugState>('debug_continue');
      if (state) {
        setDebugState(state);
        addOutput(`  PC = ${state.pc}`);
      }
    } catch (err) {
      addOutput(`  Error: ${err}`);
    }
  }, [connected, addOutput]);

  const handleStop = useCallback(async () => {
    if (!connected) return;
    try { await safeInvoke('debug_stop'); } catch { /* ignore */ }
    setConnected(false);
    setDebugState(null);
    setBreakpoints([]);
    setWatchedVars([]);
    addOutput('\n--- Execution stopped ---\n');
  }, [connected, addOutput]);

  const handleStepOver = useCallback(async () => {
    if (!connected) return;
    try {
      const state = await safeInvoke<DebugState>('debug_step_over');
      if (state) {
        setDebugState(state);
        addOutput(`  Step over → PC = ${state.pc} | ${state.stack[0]?.function || '?'}`);
      }
    } catch (err) {
      addOutput(`  Step over error: ${err}`);
    }
  }, [connected, addOutput]);

  const handleStepInto = useCallback(async () => {
    if (!connected) return;
    try {
      const state = await safeInvoke<DebugState>('debug_step_into');
      if (state) {
        setDebugState(state);
        addOutput(`  Step into → PC = ${state.pc} | ${state.stack[0]?.function || '?'}`);
      }
    } catch (err) {
      addOutput(`  Step into error: ${err}`);
    }
  }, [connected, addOutput]);

  const handleStepOut = useCallback(async () => {
    if (!connected) return;
    try {
      const state = await safeInvoke<DebugState>('debug_step_out');
      if (state) {
        setDebugState(state);
        addOutput(`  ${t('debug.stepOutResult', { pc: state.pc, func: state.stack[0]?.function || '?' })}`);
      }
    } catch (err) {
      addOutput(`  ${t('debug.stepOutError', { error: String(err) })}`);
    }
  }, [connected, addOutput]);

  const handleDeleteBreakpoint = useCallback(async (id: number) => {
    if (!connected) return;
    try {
      await safeInvoke('debug_delete_breakpoint', { id });
      setBreakpoints((prev) => prev.filter((b) => b.id !== id));
      addOutput(`  Breakpoint #${id} deleted`);
    } catch (err) {
      addOutput(`  Failed to delete breakpoint: ${err}`);
    }
  }, [connected, addOutput]);

  const handleAddWatch = useCallback(async () => {
    const name = watchInput.trim();
    if (!name || !connected) return;
    try {
      const info = await safeInvoke<VariableInfo>('debug_read_variable', { name });
      if (info) {
        setWatchedVars((prev) => {
          const filtered = prev.filter((v) => v.name !== name);
          return [...filtered, info];
        });
        addOutput(`  ${info.type_name} ${info.name} = ${info.value}`);
      }
      setWatchInput('');
    } catch (err) {
      addOutput(`  Read ${name}: ${err}`);
    }
  }, [watchInput, connected, addOutput]);

  const handleRemoveWatch = useCallback((name: string) => {
    setWatchedVars((prev) => prev.filter((v) => v.name !== name));
  }, []);

  const handleRefreshWatches = useCallback(async () => {
    if (!connected) return;
    const updated: VariableInfo[] = [];
    for (const v of watchedVars) {
      try {
        const info = await safeInvoke<VariableInfo>('debug_read_variable', { name: v.name });
        if (info) updated.push(info);
      } catch { updated.push(v); }
    }
    setWatchedVars(updated);
  }, [connected, watchedVars]);

  const subTabs: { key: SubTab; label: string }[] = [
    { key: 'console', label: t('debug.console') },
    { key: 'breakpoints', label: `${t('debug.breakpoints')}${breakpoints.length ? ` (${breakpoints.length})` : ''}` },
    { key: 'variables', label: `${t('debug.watch')}${watchedVars.length ? ` (${watchedVars.length})` : ''}` },
    { key: 'stack', label: t('debug.stack') },
    { key: 'registers', label: t('debug.registers') },
  ];

  const btnBase = 'px-1.5 py-0.5 text-[11px] rounded-sm transition-colors flex items-center gap-1';

  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center gap-0.5 px-2 py-1.5 border-b border-border-subtle shrink-0 flex-wrap">
        <DebugBtn icon={<Play size={10} />} label="Continue" disabled={!connected} onClick={handleContinue} />
        <DebugBtn icon={<Square size={10} />} label="Stop" disabled={!connected} onClick={handleStop} />
        <div className="w-px h-4 bg-border-default mx-0.5" />
        <DebugBtn icon={<CornerDownRight size={10} />} label="Step Into" disabled={!connected} onClick={handleStepInto} />
        <DebugBtn icon={<ArrowDown size={10} />} label="Step Over" disabled={!connected} onClick={handleStepOver} />
        <DebugBtn icon={<ArrowUpRight size={10} />} label="Step Out" disabled={!connected} onClick={handleStepOut} />
        <div className="flex-1" />
        <input
          className="w-40 px-1.5 py-0.5 text-[11px] bg-surface-root border border-border-subtle rounded-sm text-text-primary placeholder:text-text-tertiary"
          placeholder={t('debug.elfPlaceholder')}
          value={elfPath}
          onChange={(e) => setElfPath(e.target.value)}
          disabled={connected}
        />
        <button
          onClick={handleAttach}
          className={`px-2 py-0.5 text-[11px] font-medium rounded-sm transition-all ${
            connected
              ? 'bg-error text-white hover:bg-red-600'
              : 'bg-success text-white hover:bg-green-600'
          }`}
        >
          {connected ? t('debug.detach') : t('debug.attach')}
        </button>
      </div>

      <div className="flex items-center gap-0 px-1 border-b border-border-subtle shrink-0">
        {subTabs.map((st) => (
          <button
            key={st.key}
            onClick={() => setSubTab(st.key)}
            className={`px-2 py-1 text-[11px] border-b-2 transition-colors ${
              subTab === st.key
                ? 'border-accent text-text-primary'
                : 'border-transparent text-text-tertiary hover:text-text-secondary'
            }`}
          >
            {st.label}
          </button>
        ))}
        <div className="flex-1" />
        {connected && (
          <button
            onClick={async () => {
              await refreshState();
              await refreshBreakpoints();
            }}
            className={`${btnBase} text-text-tertiary hover:text-text-primary`}
            title={t('debug.refresh')}
          >
            <RefreshCw size={10} />
          </button>
        )}
        {subTab === 'console' && (
          <button
            onClick={() => setOutput([])}
            className={`${btnBase} text-text-tertiary hover:text-text-primary`}
          >
            <Trash2 size={10} />
          </button>
        )}
      </div>

      {subTab === 'console' && (
        <div
          ref={outputRef}
          className="flex-1 bg-surface-root p-2 overflow-y-auto font-mono text-[11px] leading-relaxed"
        >
          {output.length === 0 && (
            <div className="text-text-tertiary select-none">
              {connected ? t('debug.debuggerAttached') : t('debug.enterElfToDebug')}
            </div>
          )}
          {output.map((line, i) => (
            <div key={i} className="whitespace-pre-wrap break-all text-text-secondary">
              {line}
            </div>
          ))}
        </div>
      )}

      {subTab === 'breakpoints' && (
        <div className="flex-1 overflow-y-auto">
          {breakpoints.length === 0 ? (
            <div className="p-3 text-text-tertiary text-[11px]">
              {connected ? t('debug.noBreakpoints') : t('debug.notConnected')}
            </div>
          ) : (
            <div className="divide-y divide-border-subtle">
              {breakpoints.map((bp) => (
                <div key={bp.id} className="flex items-center gap-2 px-2 py-1.5 text-[11px] hover:bg-surface-hover">
                  <span className={`w-1.5 h-1.5 rounded-full ${bp.enabled ? 'bg-error' : 'bg-text-tertiary'}`} />
                  <span className="text-text-tertiary font-mono">#{bp.id}</span>
                  <span className="text-text-secondary truncate flex-1">{bp.file}:{bp.line}</span>
                  <span className="text-text-tertiary font-mono text-[10px]">{bp.address}</span>
                  {bp.hit_count > 0 && (
                    <span className="text-text-tertiary text-[10px]">×{bp.hit_count}</span>
                  )}
                  <button
                    onClick={() => handleDeleteBreakpoint(bp.id)}
                    className="text-text-tertiary hover:text-error transition-colors"
                  >
                    <X size={12} />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {subTab === 'variables' && (
        <div className="flex-1 flex flex-col">
          <div className="flex items-center gap-1 px-2 py-1 border-b border-border-subtle">
            <input
              className="flex-1 px-1.5 py-0.5 text-[11px] bg-surface-root border border-border-subtle rounded-sm text-text-primary placeholder:text-text-tertiary"
              placeholder={t('debug.variablePlaceholder')}
              value={watchInput}
              onChange={(e) => setWatchInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleAddWatch()}
            />
            <button
              onClick={handleAddWatch}
              disabled={!connected || !watchInput.trim()}
              className={`${btnBase} ${connected && watchInput.trim() ? 'text-accent hover:bg-surface-hover' : 'text-text-tertiary'} disabled:opacity-40`}
            >
              <Plus size={10} /> Watch
            </button>
            <button
              onClick={handleRefreshWatches}
              disabled={!connected || watchedVars.length === 0}
              className={`${btnBase} text-text-tertiary hover:text-text-primary disabled:opacity-40`}
            >
              <RefreshCw size={10} />
            </button>
          </div>
          <div className="flex-1 overflow-y-auto">
            {watchedVars.length === 0 ? (
              <div className="p-3 text-text-tertiary text-[11px]">
                {connected ? t('debug.addWatchAbove') : t('debug.notConnected')}
              </div>
            ) : (
              <div className="divide-y divide-border-subtle">
                {watchedVars.map((v) => (
                  <div key={v.name} className="flex items-center gap-2 px-2 py-1.5 text-[11px] hover:bg-surface-hover group">
                    <span className="text-text-tertiary font-mono text-[10px]">{v.type_name}</span>
                    <span className="text-accent font-mono">{v.name}</span>
                    <span className="text-text-tertiary">=</span>
                    <span className="text-text-primary font-mono flex-1 truncate">{v.value}</span>
                    <button
                      onClick={() => handleRemoveWatch(v.name)}
                      className="text-text-tertiary hover:text-error opacity-0 group-hover:opacity-100 transition-all"
                    >
                      <X size={12} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {subTab === 'stack' && (
        <div className="flex-1 overflow-y-auto">
          {!debugState || debugState.stack.length === 0 ? (
            <div className="p-3 text-text-tertiary text-[11px]">
              {connected ? t('debug.noStackFrames') : t('debug.notConnected')}
            </div>
          ) : (
            <div className="divide-y divide-border-subtle">
              {debugState.stack.map((frame) => (
                <div key={frame.level} className="flex items-center gap-2 px-2 py-1.5 text-[11px] cursor-pointer hover:bg-surface-hover">
                  <span className="text-text-tertiary font-mono w-6 text-right">#{frame.level}</span>
                  <span className="text-accent font-mono">{frame.function}</span>
                  <span className="text-text-tertiary truncate flex-1">{frame.file}:{frame.line}</span>
                  <span className="text-text-tertiary font-mono text-[10px]">{frame.address}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {subTab === 'registers' && (
        <div className="flex-1 overflow-y-auto">
          {!debugState || debugState.registers.length === 0 ? (
            <div className="p-3 text-text-tertiary text-[11px]">
              {connected ? t('debug.noRegisterData') : t('debug.notConnected')}
            </div>
          ) : (
            <div className="p-1.5 grid grid-cols-2 gap-0.5">
              <div className="px-2 py-0.5 text-text-secondary text-[11px] font-medium border-b border-border-subtle">
                PC: <span className="font-mono text-accent">{debugState.pc}</span>
              </div>
              <div className="px-2 py-0.5 border-b border-border-subtle" />
              {debugState.locals.length > 0 && (
                <>
                  <div className="px-2 py-0.5 text-text-secondary text-[11px] font-medium border-b border-border-subtle col-span-2 mt-1 -mb-1">
                    {t('debug.locals')}
                  </div>
                  {debugState.locals.map(([name, value]) => (
                    <div key={name} className="flex items-center gap-2 px-2 py-0.5 text-[11px] hover:bg-surface-hover">
                      <span className="text-success font-mono w-24 truncate">{name}</span>
                      <span className="text-text-primary font-mono">{value}</span>
                    </div>
                  ))}
                  <div className="px-2 py-0.5 text-text-secondary text-[11px] font-medium border-b border-border-subtle col-span-2 mt-1 -mb-1">
                    {t('debug.registers')}
                  </div>
                </>
              )}
              {debugState.registers.map(([name, value]) => (
                <div key={name} className="flex items-center gap-2 px-2 py-0.5 text-[11px] hover:bg-surface-hover">
                  <span className="text-accent font-mono w-12">{name}</span>
                  <span className="text-text-primary font-mono">{value}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function DebugBtn({
  icon, label, disabled, onClick,
}: {
  icon: React.ReactNode; label: string; disabled: boolean; onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`px-1.5 py-0.5 rounded-sm text-[11px] font-medium transition-all flex items-center gap-1 ${
        disabled
          ? 'text-text-tertiary cursor-not-allowed'
          : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary active:bg-surface-overlay'
      }`}
    >
      {icon}
      {label}
    </button>
  );
}