import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { Search, File, X } from 'lucide-react';
import { safeInvoke } from '../../lib/invoke';
import { useFileStore, useProjectStore } from '../../stores';

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

interface QuickOpenDialogProps {
  onClose: () => void;
}

export function QuickOpenDialog({ onClose }: QuickOpenDialogProps) {
  const [query, setQuery] = useState('');
  const [files, setFiles] = useState<string[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const { currentProject } = useProjectStore();

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!currentProject?.path) return;
    const collected: string[] = [];
    let cancelled = false;

    async function collectFiles(dirPath: string) {
      try {
        const entries = await safeInvoke<FileEntry[]>('list_directory', { path: dirPath });
        if (!entries || cancelled) return;
        for (const entry of entries) {
          if (entry.is_dir) {
            await collectFiles(entry.path);
          } else {
            collected.push(entry.path);
          }
        }
      } catch { /* ignore */ }
    }

    collectFiles(currentProject.path).then(() => {
      if (!cancelled) setFiles(collected);
    });

    return () => { cancelled = true; };
  }, [currentProject]);

  const filtered = useMemo(() => {
    if (!query.trim()) return files;
    const lower = query.toLowerCase();
    return files
      .map((path) => {
        const name = path.split(/[\\/]/).pop()?.toLowerCase() || '';
        const idx = name.indexOf(lower);
        if (idx === -1) {
          let qi = 0;
          for (let i = 0; i < name.length && qi < lower.length; i++) {
            if (name[i] === lower[qi]) qi++;
          }
          if (qi === lower.length) return { path, score: 100 + name.length };
          return null;
        }
        return { path, score: idx };
      })
      .filter(Boolean)
      .sort((a, b) => a!.score - b!.score)
      .map((item) => item!.path);
  }, [query, files]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [filtered.length]);

  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const item = list.children[selectedIndex] as HTMLElement;
    if (item) item.scrollIntoView({ block: 'nearest' });
  }, [selectedIndex]);

  const handleSelect = useCallback((path: string) => {
    useFileStore.getState().openFile(path);
    onClose();
  }, [onClose]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setSelectedIndex((prev) => Math.min(prev + 1, filtered.length - 1));
        break;
      case 'ArrowUp':
        e.preventDefault();
        setSelectedIndex((prev) => Math.max(prev - 1, 0));
        break;
      case 'Enter':
        e.preventDefault();
        if (filtered[selectedIndex]) handleSelect(filtered[selectedIndex]);
        break;
      case 'Escape':
        e.preventDefault();
        onClose();
        break;
    }
  }, [filtered, selectedIndex, handleSelect, onClose]);

  const getFileName = (path: string) => path.split(/[\\/]/).pop() || path;
  const getDirPath = (path: string) => {
    const parts = path.split(/[\\/]/);
    parts.pop();
    return parts.join('/');
  };

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-[15vh]" onClick={onClose}>
      <div
        className="w-[560px] max-h-[400px] bg-surface-elevated border border-border-default rounded-lg shadow-2xl overflow-hidden flex flex-col"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="flex items-center gap-2 px-3 py-2.5 border-b border-border-default">
          <Search size={14} className="text-text-tertiary shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search files by name..."
            className="flex-1 bg-transparent text-[13px] text-text-primary placeholder:text-text-tertiary outline-none"
          />
          <button onClick={onClose} className="text-text-tertiary hover:text-text-primary">
            <X size={14} />
          </button>
        </div>
        <div ref={listRef} className="flex-1 overflow-y-auto py-1">
          {filtered.length === 0 ? (
            <div className="px-3 py-6 text-center text-[12px] text-text-tertiary">
              {files.length === 0 ? 'No files in project' : 'No matching files'}
            </div>
          ) : (
            filtered.slice(0, 100).map((path, index) => (
              <button
                key={path}
                className={`w-full flex items-center gap-2 px-3 py-1.5 text-left text-[12px] transition-colors ${
                  index === selectedIndex
                    ? 'bg-accent/15 text-text-primary'
                    : 'text-text-secondary hover:bg-surface-hover'
                }`}
                onClick={() => handleSelect(path)}
                onMouseEnter={() => setSelectedIndex(index)}
              >
                <File size={13} className="text-text-tertiary shrink-0" />
                <span className="font-medium">{getFileName(path)}</span>
                <span className="text-text-tertiary truncate ml-auto text-[11px]">
                  {currentProject ? getDirPath(path).replace(currentProject.path, '').replace(/^[\\/]/, '') : getDirPath(path)}
                </span>
              </button>
            ))
          )}
        </div>
        {filtered.length > 100 && (
          <div className="px-3 py-1.5 text-[10px] text-text-tertiary border-t border-border-default">
            Showing 100 of {filtered.length} results
          </div>
        )}
      </div>
    </div>
  );
}
