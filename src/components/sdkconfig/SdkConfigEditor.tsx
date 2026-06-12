import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { SearchBar } from './components/SearchBar';
import { SettingsTree } from './components/SettingsTree';
import { ConfigElement } from './components/ConfigElement';
import { Menu, menuType, KconfigResponse, rawToMenu, applyValues } from './Menu';
import { safeInvoke } from '../../lib/invoke';
import { showToast } from '../ui/Toast';
import styles from './SdkConfigEditor.module.css';

interface SdkConfigEditorProps {
  projectPath: string;
  idfPath: string;
  onClose: () => void;
}

function filterItems(items: Menu[], searchString: string): Menu[] {
  const filtered: Menu[] = [];
  items.forEach((item) => {
    if (item.isVisible === false) return;
    const nameMatch = item.name && item.name.toLowerCase().indexOf(searchString) >= 0;
    const titleMatch = item.title && item.title.toLowerCase().indexOf(searchString) >= 0;
    if (nameMatch || titleMatch) {
      filtered.push(item);
    } else {
      const filteredChildren = filterItems(item.children, searchString);
      if (filteredChildren.length > 0) {
        const newItem = { ...item };
        if (item.type !== menuType.choice) newItem.children = filteredChildren;
        filtered.push(newItem);
      }
    }
  });
  return filtered;
}

export function SdkConfigEditor({ projectPath, idfPath, onClose }: SdkConfigEditorProps) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [items, setItems] = useState<Menu[]>([]);
  const [searchString, setSearchString] = useState('');
  const [selectedMenu, setSelectedMenu] = useState('');
  const [isDragging, setIsDragging] = useState(false);
  const [treeWidth, setTreeWidth] = useState(400);
  const [confserverVersion] = useState(2);

  const minTreeWidth = 300;
  const maxTreeWidth = 600;
  const scrollableRef = useRef<HTMLDivElement>(null);

  // --- Data loading via confserver ---
  const loadConfig = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await safeInvoke<any>('sdkconfig_load', { projectPath, idfPath }) as KconfigResponse;
      if (!data || !data.menus) throw new Error('SDK config returned empty menu tree');
      const menus = rawToMenu(data.menus);
      const withValues = applyValues(menus, data.values || {});
      console.log(`[SdkConfig] Loaded ${data.menus?.length} menus, ${Object.keys(data.values || {}).length} values (confserver)`);
      setItems(withValues);
    } catch (err: any) {
      console.error('[SdkConfigEditor] load error:', err);
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [projectPath, idfPath]);

  useEffect(() => { loadConfig(); }, [loadConfig]);

  // --- Close confserver on unmount ---
  useEffect(() => {
    return () => {
      safeInvoke('sdkconfig_close', {}).catch(() => {});
    };
  }, []);

  // --- Computed ---
  const displayItems = useMemo(() => {
    if (searchString !== '') {
      const m = /^(?:CONFIG_)?(.+)/.exec(searchString);
      const sm = m && m.length > 1 ? m[1].toLowerCase() : searchString.toLowerCase();
      return filterItems(items, sm);
    }
    return items;
  }, [items, searchString]);

  const lastVisibleRootIndex = useMemo(() => {
    const arr = displayItems || [];
    for (let i = arr.length - 1; i >= 0; i--) {
      if (arr[i]?.isVisible !== false) return i;
    }
    return -1;
  }, [displayItems]);

  // --- Value change: update React state AND notify confserver ---
  const handleValueChange = useCallback(
    (config: Menu, newValue: any) => {
      // Update local state first for instant UI feedback
      setItems((prev) => {
        const update = (menus: Menu[]): Menu[] =>
          menus.map((m) => {
            if (m.id === config.id) {
              if (m.type === menuType.choice) {
                const updatedChildren = m.children.map((child) => ({
                  ...child,
                  value: child.name === newValue ? 'y' : 'n',
                }));
                return { ...m, value: newValue, children: updatedChildren };
              }
              return { ...m, value: newValue };
            }
            if (m.children.length > 0) return { ...m, children: update(m.children) };
            return m;
          });
        return update(prev);
      });

      // Notify confserver (fire-and-forget — on error, just log)
      let csKey: string;
      let csValue: any;
      if (config.type === menuType.choice) {
        csKey = newValue;  // selected child's name
        csValue = true;
      } else if (config.type === menuType.hex || config.type === menuType.string) {
        csKey = config.name;
        csValue = String(newValue);
      } else if (config.type === menuType.int) {
        csKey = config.name;
        csValue = Number(newValue);
      } else {
        csKey = config.name;
        csValue = newValue === true || newValue === 'y' || newValue === '1';
      }

      safeInvoke('sdkconfig_set_value', { key: csKey, value: csValue })
        .then((resp) => {
          if (resp) console.log(`[SdkConfig] confserver set ${csKey}=${csValue} OK`);
        })
        .catch((e) => console.warn(`[SdkConfig] confserver set failed:`, e));
    },
    []
  );

  // --- Save via confserver ---
  const handleSave = useCallback(async () => {
    try {
      await safeInvoke('sdkconfig_save', { projectPath });
      showToast('success', 'Saved SDK configuration');
    } catch (err: any) {
      console.error('[SdkConfig] Save error:', err);
      showToast('error', `Save failed: ${err}`);
    }
  }, [projectPath]);

  const handleDiscard = useCallback(() => { loadConfig(); }, [loadConfig]);
  const handleReset = useCallback(() => { showToast('info', 'Reset to defaults requested'); }, []);

  const handleResetElement = useCallback((id: string) => { showToast('info', `Reset: ${id}`); }, []);
  const handleResetChildren = useCallback(() => { showToast('info', 'Reset children'); }, []);

  const textDictionary = useMemo(() => ({ save: 'Save', discard: 'Discard', reset: 'Reset' }), []);

  // --- Scroll sync ---
  const handleScroll = useCallback(() => {
    const configList = scrollableRef.current;
    if (!configList) return;
    const sections = Array.from(configList.querySelectorAll('[id]')) as HTMLElement[];
    if (sections.length === 0) return;
    const scrollTop = configList.scrollTop;
    let current: HTMLElement | null = null;
    for (const s of sections) {
      if (s.offsetTop - configList.offsetTop <= scrollTop + 10) current = s;
      else break;
    }
    if (current && current.id && selectedMenu !== current.id) setSelectedMenu(current.id);
  }, [selectedMenu]);

  const handleMenuSelect = useCallback((value: string) => {
    setSelectedMenu(value);
    document.getElementById(value)?.scrollIntoView({ behavior: 'auto', block: 'start' });
  }, []);

  // --- Drag resize ---
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    setIsDragging(true);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
    e.preventDefault();
  }, []);

  useEffect(() => {
    if (!isDragging) return;
    const onMove = (e: MouseEvent) => {
      const main = document.getElementById('sdkconfig-main');
      if (!main) return;
      const w = e.clientX - main.getBoundingClientRect().left;
      if (w >= minTreeWidth && w <= maxTreeWidth) setTreeWidth(w);
    };
    const onUp = () => {
      setIsDragging(false);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [isDragging]);

  // --- Render ---
  if (loading) {
    return (
      <div className={styles.loadingContainer}>
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className={styles.spinner}>
          <circle cx="12" cy="12" r="10" strokeDasharray="31.4 31.4" strokeLinecap="round" />
        </svg>
        <span>Loading SDK Configuration...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className={styles.errorContainer}>
        <p className={styles.errorTitle}>Failed to load SDK Configuration</p>
        <p className={styles.errorMessage}>{error}</p>
        <div className={styles.errorButtons}>
          <button onClick={loadConfig} className={styles.btnPrimary}>Retry</button>
          <button onClick={onClose} className={styles.btnSecondary}>Close</button>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.container}>
      <div className={styles.topBar}>
        <SearchBar searchString={searchString} onSearchChange={setSearchString}
          onSave={handleSave} onDiscard={handleDiscard} onReset={handleReset}
          textDictionary={textDictionary} />
        <button className={styles.closeBtn} onClick={onClose} title="Close">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
            <path fillRule="evenodd" d="M8 7.293l4.146-4.147.707.708L8.707 8l4.146 4.146-.707.708L8 8.707l-4.146 4.147-.707-.708L7.293 8 3.147 3.854l.707-.708L8 7.293z" />
          </svg>
        </button>
      </div>
      <div id="sdkconfig-main" className={styles.gridContainer}>
        <div className={styles.sidenav} style={{ width: treeWidth, minWidth: treeWidth, maxWidth: treeWidth }}>
          <SettingsTree data={items} selectedMenu={selectedMenu} onSelect={handleMenuSelect} />
        </div>
        <div className={`${styles.resizeHandle} ${isDragging ? styles.dragging : ''}`} onMouseDown={handleMouseDown} />
        <div ref={scrollableRef} id="scrollable" className={styles.configList} onScroll={handleScroll}>
          {displayItems.map((config, index) => (
            <div key={config.id}>
              <ConfigElement config={config} onValueChange={handleValueChange}
                onResetElement={handleResetElement} onResetChildren={handleResetChildren}
                confserverVersion={confserverVersion} />
              {config.isVisible !== false && index !== lastVisibleRootIndex && <div className={styles.sectionDivider} />}
            </div>
          ))}
          {displayItems.length === 0 && (
            <div className={styles.emptyState}>{searchString ? 'No matching items' : 'No configuration items'}</div>
          )}
        </div>
      </div>
    </div>
  );
}

export default SdkConfigEditor;
