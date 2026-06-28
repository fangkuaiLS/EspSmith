import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './SearchBar.module.css';

interface SearchBarProps {
  searchString: string;
  onSearchChange: (value: string) => void;
  onSave: () => void;
  onDiscard: () => void;
  onReset: () => void;
  textDictionary: {
    save: string;
    discard: string;
    reset: string;
  };
}

export function SearchBar({
  searchString,
  onSearchChange,
  onSave,
  onDiscard,
  onReset,
  textDictionary,
}: SearchBarProps) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  return (
    <div className={styles.searchContainer}>
      <div className={styles.searchWrapper}>
        <input
          ref={inputRef}
          type="search"
          name="search"
          placeholder={t('sdkconfig.searchPlaceholder')}
          autoComplete="off"
          className={styles.searchInput}
          value={searchString}
          onChange={(e) => onSearchChange(e.target.value)}
        />
      </div>
      <div className={styles.buttonGroup}>
        <button className={styles.vscodeButton} onClick={onSave}>
          {textDictionary.save}
        </button>
        <button className={styles.vscodeButton} onClick={onDiscard}>
          {textDictionary.discard}
        </button>
        <button className={styles.vscodeButton} onClick={onReset}>
          {textDictionary.reset}
        </button>
      </div>
    </div>
  );
}