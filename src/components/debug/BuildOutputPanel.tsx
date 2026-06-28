import { useRef, useEffect, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import { useFileStore, useProjectStore } from '../../stores';

interface BuildOutputPanelProps {
  output: string[];
  onClear: () => void;
  isBuilding: boolean;
}

/** GCC/CMake 诊断行解析结果 */
interface Diagnostic {
  file: string;
  line: number;
  column?: number;
  severity: 'error' | 'warning' | 'note';
}

/**
 * 尝试从构建输出行中解析出 GCC/CMake 诊断信息。
 * 支持格式：
 *   - `path/file.c:123:45: error: ...`
 *   - `path/file.c:123: error: ...`
 *   - `path/file.c(123): error: ...`  (MSVC-like)
 */
function parseDiagnostic(line: string): Diagnostic | null {
  // 匹配 `file:line:col: severity:` 或 `file:line: severity:`
  const m = line.match(/^(\S+?):(\d+)(?::(\d+))?:\s*(error|warning|note|fatal error):\s/i);
  if (!m) return null;
  const [, file, lineStr, colStr, severityStr] = m;
  const severity = severityStr.toLowerCase().includes('error') ? 'error'
    : severityStr.toLowerCase().includes('warning') ? 'warning'
    : 'note';
  return {
    file,
    line: parseInt(lineStr, 10),
    column: colStr ? parseInt(colStr, 10) : undefined,
    severity: severity as Diagnostic['severity'],
  };
}

function BuildOutputPanel({ output, onClear, isBuilding }: BuildOutputPanelProps) {
  const { t } = useTranslation();
  const endRef = useRef<HTMLDivElement>(null);
  const openFile = useFileStore((s) => s.openFile);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [output]);

  /** 点击错误行时跳转到源码 */
  const handleLineClick = useCallback(async (line: string) => {
    const diag = parseDiagnostic(line);
    if (!diag) return;

    // 尝试解析为绝对路径：如果是相对路径，拼接项目根目录
    const projectPath = useProjectStore.getState().currentProject?.path;
    let filePath = diag.file;
    if (projectPath && !filePath.match(/^[A-Z]:[\\/]/i) && !filePath.startsWith('/')) {
      filePath = `${projectPath}/${filePath}`;
    }

    try {
      await openFile(filePath);
      // 跳转到指定行（延迟等待编辑器加载）
      setTimeout(() => {
        // 通过自定义事件通知 CodeEditor 跳转
        window.dispatchEvent(new CustomEvent('editor-goto-line', {
          detail: { line: diag.line, column: diag.column }
        }));
      }, 200);
    } catch (err) {
      console.warn('Failed to open file from build output:', err);
    }
  }, [openFile]);

  /** 获取行的 CSS 类名 */
  function getLineClass(line: string): string {
    const diag = parseDiagnostic(line);
    if (diag) {
      if (diag.severity === 'error') return 'text-error cursor-pointer hover:bg-surface-hover';
      if (diag.severity === 'warning') return 'text-warning cursor-pointer hover:bg-surface-hover';
      return 'text-text-tertiary cursor-pointer hover:bg-surface-hover';
    }
    // 回退：按 emoji/关键词着色
    if (line.includes('❌') || line.includes('error:')) return 'text-error';
    if (line.includes('✅')) return 'text-success';
    if (line.includes('⚠️') || line.includes('warning:')) return 'text-warning';
    return 'text-text-secondary';
  }

  return (
    <div className="h-full flex flex-col p-3 gap-2">
      <div className="flex items-center gap-2 shrink-0">
        {isBuilding && (
          <span className="flex items-center gap-1.5 text-[11px] text-accent">
            <Loader2 size={12} className="animate-spin" />
            {t('bottomPanel.processing')}
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
          output.map((line, i) => {
            const diag = parseDiagnostic(line);
            return (
              <div
                key={i}
                className={`whitespace-pre-wrap break-all rounded-sm px-1 ${getLineClass(line)}`}
                onClick={diag ? () => handleLineClick(line) : undefined}
                title={diag ? `${diag.file}:${diag.line}${diag.column ? ':' + diag.column : ''}` : undefined}
              >
                {line}
              </div>
            );
          })
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}
const BuildOutputPanelMemo = memo(BuildOutputPanel);
export { BuildOutputPanelMemo as BuildOutputPanel };
