import { useState, useCallback, useMemo } from 'react';
import { Menu, menuType } from '../Menu';
import styles from './SettingsTree.module.css';

interface TreeItem {
  id: string;
  label: string;
  value: string;
  open: boolean;
  subItems: TreeItem[];
  isVisible?: boolean;
}

interface SettingsTreeProps {
  data: Menu[];
  selectedMenu: string;
  onSelect: (value: string) => void;
}

export function SettingsTree({ data, selectedMenu, onSelect }: SettingsTreeProps) {
  const [openStates, setOpenStates] = useState<Record<string, boolean>>({});

  const processMenuItems = useCallback(
    (items: Menu[]): TreeItem[] => {
      return items
        .filter((item) => item.type === menuType.menu && item.isVisible !== false)
        .map((item) => ({
          id: item.id,
          label: item.title,
          value: item.id,
          get open() {
            return openStates[item.id] ?? false;
          },
          isVisible: item.isVisible,
          subItems: item.children ? processMenuItems(item.children) : [],
        }));
    },
    [openStates]
  );

  const treeData = useMemo(() => (data ? processMenuItems(data) : []), [data, processMenuItems]);

  function toggleItem(item: TreeItem) {
    setOpenStates((prev) => ({ ...prev, [item.id]: !prev[item.id] }));
  }

  function selectItem(item: TreeItem) {
    toggleItem(item);
    onSelect(item.value);
  }

  function renderTreeItem(item: TreeItem, depth: number) {
    const hasChildren = item.subItems && item.subItems.length > 0;
    const isSelected = selectedMenu === item.value;

    return (
      <li key={item.id} className={styles.treeItem}>
        <div className={styles.treeItemContent}>
          {hasChildren ? (
            <div className={styles.treeItemToggle} onClick={() => toggleItem(item)}>
              {item.open ? (
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
                  <path d="M6 4l4 4-4 4" />
                </svg>
              ) : (
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
                  <path d="M11 6H5v1h6V6z" />
                </svg>
              )}
            </div>
          ) : (
            <div className={styles.treeItemTogglePlaceholder} />
          )}
          <div
            className={`${styles.treeItemLabel} ${isSelected ? styles.selected : ''}`}
            data-value={item.value}
            onClick={() => selectItem(item)}
          >
            {item.label}
          </div>
        </div>
        {hasChildren && item.open && (
          <ul className={styles.treeList}>
            {item.subItems.map((subItem) => renderTreeItem(subItem, depth + 1))}
          </ul>
        )}
      </li>
    );
  }

  return (
    <div className={styles.settingsTree}>
      <ul className={styles.treeList}>
        {treeData.map((item) => renderTreeItem(item, 0))}
      </ul>
    </div>
  );
}