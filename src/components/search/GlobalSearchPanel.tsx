import { useState, useEffect, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Search, File, X, ChevronRight } from 'lucide-react';
import { safeInvoke } from '../../lib/invoke';
import { useFileStore, useProjectStore } from '../../stores';

interface SearchMatch {
  file_path: string;
  line_number: number;
  line_content: string;
}

interface GlobalSearchPanelProps {
  onClose: () => void;
}

export function GlobalSearchPanel({ onClose }: GlobalSearchPanelProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchMatch[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const { currentProject } = useProjectStore();

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const handleSearch = useCallback(async () => {
    if (!query.trim() || !currentProject?.path) return;
    setIsSearching(true);
    try {
      const matches = await safeInvoke<SearchMatch[]>('search_in_files', {
        projectPath: currentProject.path,
        query: query.trim(),
      });
      setResults(matches || []);
    } catch (err) {
      console.error('Search failed:', err);
      setResults([]);
    } finally {
      setIsSearching(false);
    }
  }, [query, currentProject]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        handleSearch();
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    },
    [handleSearch, onClose]
  );

  const grouped = results.reduce<Record<string, SearchMatch[]>>((acc, match) => {
    if (!acc[match.file_path]) acc[match.file_path] = [];
    acc[match.file_path].push(match);
    return acc;
  }, {});

  const fileNames = Object.keys(grouped);

  const getFileName = (path: string) => path.split(/[\\/]/).pop() || path;

  const handleResultClick = async (match: SearchMatch) => {
    await useFileStore.getState().openFile(match.file_path);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[10vh]"
      onClick={onClose}
    >
      <div
        className="w-[640px] max-h-[500px] bg-surface-elevated border border-border-default rounded-lg shadow-2xl overflow-hidden flex flex-col"
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
            placeholder={t('search.placeholder')}
            className="flex-1 bg-transparent text-[13px] text-text-primary placeholder:text-text-tertiary outline-none"
          />
          {isSearching && (
            <span className="text-[11px] text-text-tertiary">{t('search.searching')}</span>
          )}
          <button
            onClick={onClose}
            className="text-text-tertiary hover:text-text-primary"
          >
            <X size={14} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto">
          {results.length === 0 && query && !isSearching ? (
            <div className="px-3 py-6 text-center text-[12px] text-text-tertiary">
              {t('search.noResults')}
            </div>
          ) : results.length > 0 ? (
            <div className="py-1">
              <div className="px-3 py-1 text-[11px] text-text-tertiary">
                {t('search.resultsCount', { count: results.length, files: fileNames.length })}
              </div>
              {fileNames.map((filePath) => (
                <div key={filePath}>
                  <button
                    className="w-full flex items-center gap-2 px-3 py-1.5 text-left text-[12px] hover:bg-surface-hover"
                    onClick={() =>
                      setSelectedFile(selectedFile === filePath ? null : filePath)
                    }
                  >
                    <ChevronRight
                      size={12}
                      className={`text-text-tertiary transition-transform ${
                        selectedFile === filePath ? 'rotate-90' : ''
                      }`}
                    />
                    <File size={13} className="text-text-tertiary shrink-0" />
                    <span className="font-medium">{getFileName(filePath)}</span>
                    <span className="text-text-tertiary ml-auto text-[10px]">
                      {grouped[filePath].length === 1
                        ? t('search.matchCountOne', { n: grouped[filePath].length })
                        : t('search.matchCount', { n: grouped[filePath].length })}
                    </span>
                  </button>
                  {selectedFile === filePath &&
                    grouped[filePath].map((match, idx) => (
                      <button
                        key={idx}
                        className="w-full flex items-center gap-2 pl-8 pr-3 py-1 text-left text-[11px] hover:bg-surface-hover"
                        onClick={() => handleResultClick(match)}
                      >
                        <span className="text-text-tertiary w-8 text-right shrink-0">
                          {match.line_number}
                        </span>
                        <span className="text-text-secondary truncate font-mono">
                          {match.line_content.trim()}
                        </span>
                      </button>
                    ))}
                </div>
              ))}
            </div>
          ) : (
            <div className="px-3 py-6 text-center text-[12px] text-text-tertiary">
              {t('search.hint')}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
