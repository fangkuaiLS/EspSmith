/**
 * Menu data model — matches VS Code ESP-IDF extension's Menu class.
 * Adapted from vscode-esp-idf-extension-master/src/espIdf/menuconfig/Menu.ts
 */

export enum menuType {
  string = 'string',
  bool = 'bool',
  int = 'int',
  choice = 'choice',
  hex = 'hex',
  menu = 'menu',
}

export interface Menu {
  children: Menu[];
  help: string;
  id: string;
  name: string;
  range: number[];
  title: string;
  type: menuType;
  isVisible: boolean;
  isCollapsed: boolean;
  value: any;
  dependsOn: string;
  isMenuconfig: boolean;
  default: any;
}

/** Raw JSON from kconfig_dump.py backend */
export interface RawMenuItem {
  type: string;
  title?: string;
  help?: string;
  name?: string;
  value?: string;
  typeHint?: string;
  range?: [number, number];
  items?: RawMenuItem[];
  choices?: { name: string; title?: string; value: string }[];
}

export interface KconfigResponse {
  version: number;
  values: Record<string, string>;
  menus: RawMenuItem[];
  sdkconfigPath?: string;
}

/** Convert raw backend JSON to Menu tree with IDs */
export function rawToMenu(raw: RawMenuItem[], path = ''): Menu[] {
  return raw.map((item, i) => {
    const id = path ? `${path}-${i}` : `${i}`;
    const menuTypeHint = item.typeHint || 'string';
    let type: menuType;
    if (item.type === 'menu') {
      type = menuType.menu;
    } else if (menuTypeHint === 'choice' || item.type === 'choice') {
      type = menuType.choice;
    } else {
      type = (menuTypeHint as menuType) || menuType.string;
    }

    let children: Menu[] = [];
    if (item.items) {
      children = rawToMenu(item.items, id);
    } else if (item.choices) {
      // Convert choice options to children — each option is a bool type child
      children = item.choices.map((c, ci) => ({
        id: `${id}-${ci}`,
        name: c.name || '',
        title: c.title || c.name || '',
        help: '',
        type: menuType.bool,
        value: c.value === 'y' ? 'y' : 'n',
        default: null,
        range: [] as number[],
        isVisible: true,
        isCollapsed: false,
        isMenuconfig: false,
        dependsOn: '',
        children: [] as Menu[],
      }));
      // The choice's value is the name of the selected child (the one with value 'y')
      const selected = children.find((c) => c.value === 'y');
      if (selected) {
        item.value = selected.name;
      }
    }

    const menu: Menu = {
      id,
      name: item.name || '',
      title: item.title || item.name || '',
      help: item.help || '',
      type,
      value: item.value ?? null,
      default: null,
      range: item.range || [],
      isVisible: true,
      isCollapsed: false,
      isMenuconfig: false,
      dependsOn: '',
      children,
    };

    // Safety net: detect choice-like menus (all children are bool or y/n values) and convert to choice.
    // This handles cases where the backend Python script hasn't been updated yet
    // and Choices are emitted as plain menus with bool/y-n children.
    if (
      type === menuType.menu &&
      children.length > 1 &&
      children.every((c) => isBoolLike(c))
    ) {
      // Check that children share a common prefix (typical of Kconfig choice symbols)
      const names = children.map((c) => c.name).filter(Boolean);
      const prefix = names.length >= 2 ? getCommonPrefix(names) : '';
      console.log(`[Menu] SafetyNet: "${menu.title}" (${menu.name}) has ${children.length} bool-like kids, prefix="${prefix}" (len=${prefix.length})`);
      if (names.length >= 2 && prefix.length >= 4) {
        menu.type = menuType.choice;
        // Find selected child (value = 'y' or true)
        const selected = children.find(
          (c) => c.value === 'y' || c.value === true || c.value === '1'
        );
        menu.value = selected ? selected.name : children[0].name;
        console.log(`[Menu] SafetyNet -> converted to CHOICE, value=${menu.value}`);
      }
    }

    return menu;
  });
}

/** Get common prefix among strings */
function getCommonPrefix(strs: string[]): string {
  if (strs.length === 0) return '';
  let prefix = strs[0];
  for (let i = 1; i < strs.length; i++) {
    while (strs[i].indexOf(prefix) !== 0) {
      prefix = prefix.slice(0, -1);
      if (prefix.length === 0) return '';
    }
  }
  return prefix;
}

/** Check if a Menu item looks like a bool/y-n config value */
function isBoolLike(m: Menu): boolean {
  const v = String(m.value ?? '').toLowerCase();
  return v === 'y' || v === 'n' || v === 'true' || v === 'false' || v === '1' || v === '0';
}

/** Apply backend values to a Menu tree */
export function applyValues(menus: Menu[], values: Record<string, string>): Menu[] {
  return menus.map((m) => {
    // For choice type, find which child is selected from values
    if (m.type === menuType.choice && m.children.length > 0) {
      const updatedChildren = m.children.map((child) => {
        if (child.name && values[child.name] !== undefined) {
          return { ...child, value: values[child.name] };
        }
        return child;
      });
      // The choice's value is the name of the child with value 'y'
      const selected = updatedChildren.find((c) => c.value === 'y');
      return { ...m, value: selected ? selected.name : m.value, children: updatedChildren };
    }
    // For other types, apply value from values dict
    if (m.name && values[m.name] !== undefined && m.type !== menuType.choice) {
      let val = values[m.name];
      if (typeof val === 'string') {
        if (val.startsWith('"') && val.endsWith('"')) val = val.slice(1, -1);
      }
      return { ...m, value: val, children: applyValues(m.children, values) };
    }
    return { ...m, children: applyValues(m.children, values) };
  });
}