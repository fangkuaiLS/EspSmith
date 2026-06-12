import { useState, useEffect } from 'react';
import { Menu } from '../Menu';
import styles from './InputComponents.module.css';

interface CheckboxInputProps {
  config: Menu;
  onChange: (value: boolean) => void;
  onReset: (id: string) => void;
  canReset: boolean;
}

export function CheckboxInput({ config, onChange, onReset, canReset }: CheckboxInputProps) {
  const [isChecked, setIsChecked] = useState(config.value === 'y' || config.value === true);
  const [showHelp, setShowHelp] = useState(false);

  useEffect(() => {
    setIsChecked(config.value === 'y' || config.value === true);
  }, [config.value]);

  return (
    <div className={styles.formGroup}>
      <div className={styles.labelRow}>
        <div className={styles.checkboxWrapper}>
          <label
            className={`${styles.vscodeCheckbox} ${isChecked ? styles.checked : ''}`}
            role="checkbox"
            aria-checked={isChecked}
            onClick={() => { setIsChecked(!isChecked); onChange(!isChecked); }}
          >
            <input type="checkbox" checked={isChecked} readOnly data-config-id={config.id} />
            <span className={styles.checkIcon}>
              {isChecked && (
                <svg width="10" height="10" viewBox="0 0 16 16" fill="currentColor">
                  <path fillRule="evenodd" d="M14.431 3.323l-8.47 10-.79-.036-3.35-4.77.818-.574 2.978 4.24 8.051-9.506.764.646z" />
                </svg>
              )}
            </span>
          </label>
          <label className={styles.configLabel} onClick={() => setShowHelp((v) => !v)}>
            {config.title}
          </label>
        </div>
        <div className={styles.iconGroup}>
          <div className={styles.infoIcon} onClick={() => setShowHelp((v) => !v)} title="Toggle help">
            <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
              <path fillRule="evenodd" d="M8 1a7 7 0 100 14A7 7 0 008 1zm0 2a5 5 0 110 10A5 5 0 018 3zm-.5 3h1v1h-1V6zm0 2h1v3h-1V8z" />
            </svg>
          </div>
          {canReset && (
            <div className={`${styles.infoIcon} ${styles.resetIcon}`} onClick={() => onReset(config.id)} title="Reset to default">
              <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
                <path fillRule="evenodd" d="M12.754 3.247a5.454 5.454 0 00-9.504 1.294H2v1h5V0H5.971v2.068A6.454 6.454 0 0114.5 5.5h-1a5.45 5.45 0 00-.746-2.253zM2.5 10.5h1a5.45 5.45 0 00.746 2.253 5.454 5.454 0 009.504-1.294H14v-1H9v5.5h1.029v-2.068A6.454 6.454 0 011.5 10.5z" />
              </svg>
            </div>
          )}
        </div>
      </div>

      {showHelp && (
        <>
          <p className={styles.helpKconfigTitle}>KCONFIG Name: <span style={{ fontWeight: 900 }}>{config.name}</span></p>
          <div className={styles.content} dangerouslySetInnerHTML={{ __html: config.help }} />
        </>
      )}
    </div>
  );
}