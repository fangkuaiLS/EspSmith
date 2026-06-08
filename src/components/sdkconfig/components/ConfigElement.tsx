import { Menu, menuType } from '../Menu';
import { CheckboxInput } from './CheckboxInput';
import { NumberInput } from './NumberInput';
import { HexInput } from './HexInput';
import { StringInput } from './StringInput';
import { SelectDropdown } from './SelectDropdown';
import styles from './ConfigElement.module.css';

interface ConfigElementProps {
  config: Menu;
  onValueChange: (config: Menu, newValue: any) => void;
  onResetElement: (id: string) => void;
  onResetChildren: (children: string[]) => void;
  confserverVersion: number;
}

export function ConfigElement({
  config,
  onValueChange,
  onResetElement,
  onResetChildren,
  confserverVersion,
}: ConfigElementProps) {
  const canReset = confserverVersion >= 3;

  if (config.isVisible === false) return null;

  function handleChange(value: any) {
    onValueChange(config, value);
  }

  function handleReset(id: string) {
    onResetElement(id);
  }

  function handleResetChildren(children: string[]) {
    onResetChildren(children);
  }

  const isMenu = config.type === menuType.menu;
  const isChoice = config.type === menuType.choice;

  return (
    <div className={isMenu ? undefined : styles.configEl}>
      {/* Menu type: render title + optional menuconfig checkbox */}
      {isMenu && (
        <div id={config.id} className={styles.submenu}>
          <div className={styles.menuTitleWrapper}>
            <h4 className={styles.menuTitle}>{config.title}</h4>
            {canReset && (
              <div
                className={`${styles.resetIcon} ${styles.menuResetIcon}`}
                onClick={(e) => {
                  e.stopPropagation();
                  onResetElement(config.id);
                }}
                title="Reset to default"
              >
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
                  <path fillRule="evenodd" d="M12.754 3.247a5.454 5.454 0 00-9.504 1.294H2v1h5V0H5.971v2.068A6.454 6.454 0 0114.5 5.5h-1a5.45 5.45 0 00-.746-2.253zM2.5 10.5h1a5.45 5.45 0 00.746 2.253 5.454 5.454 0 009.504-1.294H14v-1H9v5.5h1.029v-2.068A6.454 6.454 0 011.5 10.5z" />
                </svg>
              </div>
            )}
          </div>
          {config.isMenuconfig && (
            <div className={styles.menuconfig}>
              <CheckboxInput
                config={config}
                onChange={handleChange}
                onReset={handleReset}
                canReset={canReset}
              />
            </div>
          )}
        </div>
      )}

      {/* Non-menu leaf types */}
      {isChoice && (
        <SelectDropdown
          config={config}
          onChange={handleChange}
          onResetChildren={handleResetChildren}
          canReset={canReset}
        />
      )}
      {config.type === menuType.bool && (
        <CheckboxInput
          config={config}
          onChange={handleChange}
          onReset={handleReset}
          canReset={canReset}
        />
      )}
      {config.type === menuType.int && (
        <NumberInput
          config={config}
          onChange={handleChange}
          onReset={handleReset}
          canReset={canReset}
        />
      )}
      {config.type === menuType.string && (
        <StringInput
          config={config}
          onChange={handleChange}
          onReset={handleReset}
          canReset={canReset}
        />
      )}
      {config.type === menuType.hex && (
        <HexInput
          config={config}
          onChange={handleChange}
          onReset={handleReset}
          canReset={canReset}
        />
      )}

      {/* Render children for ALL types except choice (matching official plugin) */}
      {!isChoice && config.children && config.children.length > 0 && (
        <div className={styles.configChildren}>
          {config.children.map((child) => (
            <ConfigElement
              key={child.id}
              config={child}
              onValueChange={onValueChange}
              onResetElement={onResetElement}
              onResetChildren={onResetChildren}
              confserverVersion={confserverVersion}
            />
          ))}
        </div>
      )}
    </div>
  );
}