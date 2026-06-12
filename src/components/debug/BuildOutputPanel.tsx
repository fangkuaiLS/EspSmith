import { useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';

interface BuildOutputPanelProps {
  output: string[];
  onClear: () => void;
  isBuilding: boolean;
}

export function BuildOutputPanel({ output, onClear, isBuilding }: BuildOutputPanelProps) {
  const { t } = useTranslation();
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [output]);

  return (
    <div className="h-full flex flex-col p-3 gap-2">
      <div className="flex items-center gap-2 shrink-0">
        {isBuilding && (
          <span className="flex items-center gap-1.5 text-[11px] text-accent">
            <Loader2 size={12} className="animate-spin" />
            Processing...
          </span>
        )}
        <div className="flex-1" />
        <button
          className="px-2 py-1 text-[11px] bg-surface-overlay border border-border-subtle rounded-sm text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors"
          onClick={onClear}
        >
          {t('bottomPanel.clear')}
        </button>
      </div>

      <div className="flex-1 bg-surface-root rounded-sm border border-border-subtle p-2.5 overflow-y-auto font-mono text-[12px] leading-relaxed">
        {output.length === 0 ? (
          <div className="text-text-tertiary select-none">
            {t('bottomPanel.buildHint')}
          </div>
        ) : (
          output.map((line, i) => (
            <div
              key={i}
              className={`whitespace-pre-wrap break-all ${
                line.includes('❌') || line.includes('error:')
                  ? 'text-error'
                  : line.includes('✅')
                  ? 'text-success'
                  : line.includes('⚠️')
                  ? 'text-warning'
                  : 'text-text-secondary'
              }`}
            >
              {line}
            </div>
          ))
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}