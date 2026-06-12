import { useRef, useEffect, useMemo, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Usb, AlertTriangle, Search, ChevronDown, X } from 'lucide-react';

const CRASH_PATTERNS = [
  'Guru Meditation Error', 'abort() was called', 'assert failed:',
  'PANIC', 'Backtrace:', 'Rebooting...', 'LoadProhibited', 'StoreProhibited',
  'IllegalInstruction', 'DivideByZero', 'Stack canary', 'Brownout',
  'Core', 'register dump', 'rst:',
];

const LINE_HEIGHT = 18;
const OVERSCAN = 20;

function isCrashLine(line: string): boolean {
  return CRASH_PATTERNS.some((p) => line.includes(p));
}

function hasCrash(output: string[]): boolean {
  return output.some((line) => isCrashLine(line));
}

interface SerialMonitorPanelProps {
  output: string[];
  input: string;
  connected: boolean;
  port: string;
  baudRate: string;
  onInputChange: (v: string) => void;
  onSend: () => void;
  onConnect: () => void;
  onBaudRateChange: (v: string) => void;
  onClear: () => void;
}

export function SerialMonitorPanel({
  output, input, connected, port, baudRate,
  onInputChange, onSend, onConnect, onBaudRateChange, onClear,
}: SerialMonitorPanelProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  const [searchText, setSearchText] = useState('');
  const [scrollTop, setScrollTop] = useState(0);
  const [containerHeight, setContainerHeight] = useState(600);

  const crashDetected = useMemo(() => hasCrash(output), [output]);

  const filteredIndices = useMemo(() => {
    if (!searchText) return null;
    const lower = searchText.toLowerCase();
    const indices: number[] = [];
    for (let i = 0; i < output.length; i++) {
      if (output[i].toLowerCase().includes(lower)) {
        indices.push(i);
      }
    }
    return indices;
  }, [output, searchText]);

  const displayLines = useMemo(() => {
    if (filteredIndices) {
      return filteredIndices.map(i => ({ originalIndex: i, text: output[i] }));
    }
    return output.map((text, i) => ({ originalIndex: i, text }));
  }, [output, filteredIndices]);

  const totalHeight = displayLines.length * LINE_HEIGHT;
  const startIndex = Math.max(0, Math.floor(scrollTop / LINE_HEIGHT) - OVERSCAN);
  const visibleCount = Math.ceil(containerHeight / LINE_HEIGHT) + OVERSCAN * 2;
  const endIndex = Math.min(displayLines.length, startIndex + visibleCount);
  const visibleLines = displayLines.slice(startIndex, endIndex);

  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    setScrollTop(el.scrollTop);
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    setFollowing(atBottom);
  }, []);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver(entries => {
      for (const entry of entries) {
        setContainerHeight(entry.contentRect.height);
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (following && containerRef.current) {
      const el = containerRef.current;
      el.scrollTop = el.scrollHeight - el.clientHeight;
    }
  }, [output, following]);

  const matchCount = filteredIndices?.length ?? 0;

  return (
    <div className="h-full flex flex-col p-3 gap-2">
      <div className="flex items-center gap-2 shrink-0">
        <div className="flex items-center gap-1 px-2 py-1 text-[11px] bg-surface-overlay border border-border-subtle rounded-sm text-text-secondary font-mono">
          <Usb size={12} />
          <span>{port || t('bottomPanel.selectPort')}</span>
        </div>
        <select
          value={baudRate}
          onChange={(e) => onBaudRateChange(e.target.value)}
          className="px-2 py-1 text-[11px] bg-surface-overlay border border-border-subtle rounded-sm text-text-secondary font-mono focus:outline-none"
        >
          <option value="9600">9600</option>
          <option value="115200">115200</option>
          <option value="460800">460800</option>
          <option value="921600">921600</option>
        </select>
        <button
          onClick={onConnect}
          className={`px-3 py-1 text-[11px] font-medium rounded-sm transition-all ${
            connected
              ? 'bg-error text-white hover:bg-red-600'
              : 'bg-success text-white hover:bg-green-600'
          }`}
        >
          {connected ? t('bottomPanel.disconnect') : t('bottomPanel.connect')}
        </button>
        <div className="flex-1" />

        <div className="flex items-center gap-1 px-1.5 py-0.5 text-[11px] bg-surface-overlay border border-border-subtle rounded-sm">
          <Search size={11} className="text-text-tertiary" />
          <input
            type="text"
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
            placeholder="Filter..."
            className="w-24 text-[11px] bg-transparent text-text-secondary placeholder:text-text-disabled focus:outline-none"
          />
          {searchText && (
            <>
              <span className="text-[10px] text-text-tertiary">{matchCount}</span>
              <button onClick={() => setSearchText('')} className="text-text-tertiary hover:text-text-primary">
                <X size={10} />
              </button>
            </>
          )}
        </div>

        <button
          onClick={() => { setFollowing(true); }}
          className={`p-1 rounded-sm transition-colors ${
            following ? 'text-accent bg-accent/10' : 'text-text-tertiary hover:text-text-primary'
          }`}
          title="Follow output"
        >
          <ChevronDown size={14} />
        </button>

        <button
          className="px-2 py-1 text-[11px] bg-surface-overlay border border-border-subtle rounded-sm text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors"
          onClick={onClear}
        >
          {t('bottomPanel.clear')}
        </button>
      </div>

      <div className="flex-1 relative bg-surface-root rounded-sm border border-border-subtle overflow-hidden">
        {crashDetected && (
          <div className="absolute top-0 left-0 right-0 z-10 flex items-center gap-2 px-2 py-1 bg-error/10 border-b border-error/30 text-error text-[11px] font-medium">
            <AlertTriangle size={12} />
            Crash detected — check GDB backtrace for details
          </div>
        )}
        <div
          ref={containerRef}
          onScroll={handleScroll}
          className="h-full overflow-y-auto font-mono text-[12px]"
        >
          {displayLines.length === 0 ? (
            <div className="p-2.5 text-text-tertiary select-none">
              {connected ? t('bottomPanel.waitingForData') : t('bottomPanel.connectToView')}
            </div>
          ) : (
            <div style={{ height: totalHeight, position: 'relative' }}>
              <div style={{ position: 'absolute', top: startIndex * LINE_HEIGHT, left: 0, right: 0 }}>
                {visibleLines.map((item) => {
                  const line = item.text;
                  const cls = isCrashLine(line)
                    ? 'text-error bg-error/5'
                    : line.startsWith('> ')
                    ? 'text-accent'
                    : 'text-text-secondary';
                  return (
                    <div
                      key={item.originalIndex}
                      className={`whitespace-pre-wrap break-all px-2.5 ${cls}`}
                      style={{ height: LINE_HEIGHT, lineHeight: `${LINE_HEIGHT}px` }}
                    >
                      {line || '\u00A0'}
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </div>

      <div className="flex gap-2 shrink-0">
        <input
          type="text"
          value={input}
          onChange={(e) => onInputChange(e.target.value)}
          placeholder={t('bottomPanel.typeCommand')}
          disabled={!connected}
          className="flex-1 px-2.5 py-1.5 text-[12px] font-mono bg-surface-overlay border border-border-subtle rounded-sm text-text-primary placeholder:text-text-disabled focus:outline-none disabled:opacity-50"
          onKeyDown={(e) => e.key === 'Enter' && onSend()}
        />
        <button
          onClick={onSend}
          disabled={!connected || !input.trim()}
          className="px-4 py-1.5 text-[12px] font-medium bg-accent text-white rounded-sm hover:bg-accent-hover transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {t('bottomPanel.send')}
        </button>
      </div>
    </div>
  );
}
