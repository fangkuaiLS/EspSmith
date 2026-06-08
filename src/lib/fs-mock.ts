/**
 * 浏览器模式 Mock 文件系统
 *
 * 在非 Tauri 环境下模拟文件操作，
 * 数据存储在 localStorage 中持久化。
 */

import type { FileEntry, ProjectInfo } from '../types';

// ==================== 内存文件系统 ====================

interface FsNode {
  name: string;
  path: string;
  is_dir: boolean;
  content?: string;
  children?: Record<string, FsNode>;
}

interface FsStore {
  roots: Record<string, FsNode>; // projectPath -> root node
  projects: ProjectInfo[];
}

const STORAGE_KEY = 'espsmith-fs';

function loadFs(): FsStore {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw);
  } catch { /* ignore */ }
  return { roots: {}, projects: [] };
}

function saveFs(store: FsStore) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(store));
}

/**
 * 获取或创建项目根节点
 *
 * @param projectPath - 项目路径
 * @param existingStore - 可选，调用者的 store 引用，避免多次 loadFs 导致覆盖
 */
function getOrCreateRoot(projectPath: string, existingStore?: FsStore): FsNode {
  const store = existingStore || loadFs();
  if (!store.roots[projectPath]) {
    const name = projectPath.split(/[\\/]/).pop() || 'project';
    store.roots[projectPath] = {
      name,
      path: projectPath,
      is_dir: true,
      children: {},
    };
    // 只有自己加载的 store 才自己保存；调用者传入的由调用者统一保存
    if (!existingStore) {
      saveFs(store);
    }
  }
  return store.roots[projectPath];
}

/** 根据路径找到节点 */
function findNode(root: FsNode, targetPath: string): FsNode | null {
  // Normalize paths
  const normTarget = targetPath.replace(/\\/g, '/');
  const normRoot = root.path.replace(/\\/g, '/');

  if (normTarget === normRoot) return root;
  if (!normTarget.startsWith(normRoot + '/')) return null;

  const relative = normTarget.slice(normRoot.length + 1);
  const parts = relative.split('/');
  let current = root;

  for (const part of parts) {
    if (!current.children || !current.children[part]) return null;
    current = current.children[part];
  }
  return current;
}

/** 确保路径上的所有目录存在 */
function ensureDir(root: FsNode, targetPath: string): FsNode {
  const normTarget = targetPath.replace(/\\/g, '/');
  const normRoot = root.path.replace(/\\/g, '/');

  if (normTarget === normRoot) return root;

  const relative = normTarget.slice(normRoot.length + 1);
  const parts = relative.split('/');
  let current = root;

  for (const part of parts) {
    if (!current.children) current.children = {};
    if (!current.children[part]) {
      const fullPath = current.path.replace(/\\/g, '/') + '/' + part;
      current.children[part] = {
        name: part,
        path: fullPath.replace(/\//g, '\\'),
        is_dir: true,
        children: {},
      };
    }
    current = current.children[part];
  }
  return current;
}

// ==================== 导出的操作 ====================

/** 创建项目 */
export function mockCreateProject(config: {
  name: string;
  path: string;
  chip: string;
  idf_version: string;
  template: string;
}): ProjectInfo {
  const projectPath = `${config.path}\\${config.name}`;
  const store = loadFs();

  // 检查是否已存在
  if (store.projects.find((p) => p.path === projectPath)) {
    throw new Error(`Project already exists: ${projectPath}`);
  }

  // 创建项目根节点（传入 store 避免被后续 saveFs 覆盖）
  const root = getOrCreateRoot(projectPath, store);

  // 添加模板文件
  const files = getTemplateFiles(config.name, config.template);
  for (const file of files) {
    const filePath = `${projectPath}\\${file.name}`;
    const parentDir = projectPath;
    const dirNode = ensureDir(root, parentDir);
    if (!dirNode.children) dirNode.children = {};
    dirNode.children[file.name] = {
      name: file.name,
      path: filePath,
      is_dir: false,
      content: file.content,
    };
  }

  // 创建子目录
  const mainDir = ensureDir(root, `${projectPath}\\main`);
  if (!mainDir.children) mainDir.children = {};
  mainDir.children['CMakeLists.txt'] = {
    name: 'CMakeLists.txt',
    path: `${projectPath}\\main\\CMakeLists.txt`,
    is_dir: false,
    content: 'idf_component_register(SRCS "main.c" INCLUDE_DIRS ".")',
  };
  mainDir.children['main.c'] = {
    name: 'main.c',
    path: `${projectPath}\\main\\main.c`,
    is_dir: false,
    content: getMainTemplate(config.name),
  };

  const projectInfo: ProjectInfo = {
    name: config.name,
    path: projectPath,
    chip: config.chip,
    idf_version: config.idf_version,
    has_hardware_config: false,
  };

  store.projects.push(projectInfo);
  saveFs(store);

  return projectInfo;
}

/** 打开项目 */
export function mockOpenProject(path: string): ProjectInfo {
  const store = loadFs();
  const project = store.projects.find((p) => p.path === path);

  // 确保 root 存在（修复因旧 bug 导致 root 丢失的情况）
  const rootExisted = !!store.roots[path];
  getOrCreateRoot(path, store);

  if (!project) {
    // 如果是通过文件系统选择的目录，创建虚拟项目
    const name = path.split(/[\\/]/).pop() || 'project';
    const newProject: ProjectInfo = {
      name,
      path,
      chip: 'ESP32',
      idf_version: 'v5.1',
      has_hardware_config: false,
    };
    store.projects.push(newProject);
    saveFs(store);
    return newProject;
  }

  // 项目存在但 root 缺失（旧 bug 遗留），保存修复后的 store
  if (!rootExisted) {
    saveFs(store);
  }

  return project;
}

/** 加载项目持久化配置（芯片型号 + 串口） */
export function mockLoadProjectConfig(projectPath: string): { chip: string; target: string | null; flash_port: string | null } {
  const store = loadFs();
  const proj = store.projects.find((p) => p.path === projectPath);
  return {
    chip: proj?.chip || 'ESP32',
    target: proj?.chip || null,
    flash_port: (proj as any)?.flash_port || null,
  };
}

/** 保存项目持久化配置（芯片型号 + 串口） */
export function mockSaveProjectConfig(projectPath: string, chip?: string, target?: string, flashPort?: string) {
  const store = loadFs();
  const proj = store.projects.find((p) => p.path === projectPath);
  if (proj) {
    if (chip) (proj as any).chip = chip;
    if (target) (proj as any).target = target;
    if (flashPort) (proj as any).flash_port = flashPort;
    saveFs(store);
  }
}

/** 列出目录 */
export function mockListDirectory(path: string): FileEntry[] {
  const roots = loadFs().roots;
  // 查找匹配的 root
  for (const rootPath of Object.keys(roots)) {
    const node = findNode(roots[rootPath], path);
    if (node && node.is_dir) {
      return Object.values(node.children || {}).map((child) => ({
        name: child.name,
        path: child.path,
        is_dir: child.is_dir,
        size: child.content ? child.content.length : 0,
      }));
    }
  }
  // 如果没有找到，返回空数组（可能是新打开的目录）
  return [];
}

/** 读取文件 */
export function mockReadFile(path: string): string {
  const roots = loadFs().roots;
  for (const rootPath of Object.keys(roots)) {
    const node = findNode(roots[rootPath], path);
    if (node && !node.is_dir && node.content !== undefined) {
      return node.content;
    }
  }
  throw new Error(`File not found: ${path}`);
}

/** 写入文件 */
export function mockWriteFile(path: string, content: string): void {
  const store = loadFs();
  for (const [rootPath, root] of Object.entries(store.roots)) {
    const node = findNode(root, path);
    if (node && !node.is_dir) {
      node.content = content;
      saveFs(store);
      return;
    }
    // 文件可能还不存在，创建它
    const normPath = path.replace(/\\/g, '/');
    const normRoot = rootPath.replace(/\\/g, '/');
    if (normPath.startsWith(normRoot + '/')) {
      const relative = normPath.slice(normRoot.length + 1);
      const parts = relative.split('/');
      const fileName = parts.pop()!;
      const dirPath = normRoot + (parts.length > 0 ? '/' + parts.join('/') : '');
      const dirNode = ensureDir(root, dirPath.replace(/\//g, '\\'));
      if (!dirNode.children) dirNode.children = {};
      dirNode.children[fileName] = {
        name: fileName,
        path,
        is_dir: false,
        content,
      };
      saveFs(store);
      return;
    }
  }
  throw new Error(`Cannot write file: ${path}`);
}

/** 创建文件 */
export function mockCreateFile(parentPath: string, name: string, content: string = ''): FileEntry {
  const store = loadFs();
  for (const [, root] of Object.entries(store.roots)) {
    const parent = findNode(root, parentPath);
    if (parent && parent.is_dir) {
      const filePath = `${parentPath}\\${name}`;
      if (!parent.children) parent.children = {};
      parent.children[name] = {
        name,
        path: filePath,
        is_dir: false,
        content,
      };
      saveFs(store);
      return { name, path: filePath, is_dir: false, size: content.length };
    }
  }
  // 创建在根目录
  for (const [rootPath] of Object.entries(store.roots)) {
    const normParent = parentPath.replace(/\\/g, '/');
    const normRoot = rootPath.replace(/\\/g, '/');
    if (normParent.startsWith(normRoot)) {
      const root = store.roots[rootPath];
      const dirNode = ensureDir(root, parentPath);
      const filePath = `${parentPath}\\${name}`;
      if (!dirNode.children) dirNode.children = {};
      dirNode.children[name] = {
        name,
        path: filePath,
        is_dir: false,
        content,
      };
      saveFs(store);
      return { name, path: filePath, is_dir: false, size: content.length };
    }
  }
  throw new Error(`Cannot create file: no matching root for ${parentPath}`);
}

/** 创建文件夹 */
export function mockCreateFolder(parentPath: string, name: string): FileEntry {
  const store = loadFs();
  for (const [, root] of Object.entries(store.roots)) {
    const parent = findNode(root, parentPath);
    if (parent && parent.is_dir) {
      const folderPath = `${parentPath}\\${name}`;
      if (!parent.children) parent.children = {};
      parent.children[name] = {
        name,
        path: folderPath,
        is_dir: true,
        children: {},
      };
      saveFs(store);
      return { name, path: folderPath, is_dir: true, size: 0 };
    }
  }
  throw new Error(`Cannot create folder: no parent found for ${parentPath}`);
}

/** 重命名文件或文件夹 */
export function mockRenameFile(oldPath: string, newName: string): FileEntry {
  const store = loadFs();
  for (const [rootPath, root] of Object.entries(store.roots)) {
    const node = findNode(root, oldPath);
    if (!node) continue;

    // 检查新名称在父目录中是否已存在
    const parentPath = oldPath.replace(/\\/g, '/').split('/').slice(0, -1).join('\\') || rootPath;
    const parent = findNode(root, parentPath);
    if (parent?.children?.[newName]) {
      throw new Error(`A file named '${newName}' already exists`);
    }

    // 从旧父节点移除
    if (parent?.children) {
      const oldName = oldPath.split(/[\\/]/).pop()!;
      delete parent.children[oldName];
    }

    // 更新节点并添加到父节点
    const newPath = `${parentPath}\\${newName}`;
    node.name = newName;
    node.path = newPath;
    if (!parent) {
      // 这是根节点重命名
      store.roots[newPath] = node;
      delete store.roots[oldPath];
    } else {
      if (!parent.children) parent.children = {};
      parent.children[newName] = node;
    }
    saveFs(store);
    return { name: newName, path: newPath, is_dir: node.is_dir, size: node.content?.length || 0 };
  }
  throw new Error(`Cannot rename: file not found: ${oldPath}`);
}

/** 删除文件或文件夹（递归删除目录） */
export function mockDeleteFile(path: string): void {
  const store = loadFs();
  for (const [rootPath, root] of Object.entries(store.roots)) {
    if (rootPath === path) {
      // 删除根目录（整个项目）
      delete store.roots[rootPath];
      saveFs(store);
      return;
    }
    const parentPath = path.replace(/\\/g, '/').split('/').slice(0, -1).join('\\');
    const parent = findNode(root, parentPath);
    if (parent?.children) {
      const name = path.split(/[\\/]/).pop()!;
      if (parent.children[name]) {
        delete parent.children[name];
        saveFs(store);
        return;
      }
    }
  }
  throw new Error(`Cannot delete: file not found: ${path}`);
}

/** 复制文件 */
export function mockDuplicateFile(path: string): FileEntry {
  const store = loadFs();
  for (const [rootPath, root] of Object.entries(store.roots)) {
    const node = findNode(root, path);
    if (!node || node.is_dir) continue;

    const parentPath = path.replace(/\\/g, '/').split('/').slice(0, -1).join('\\') || rootPath;
    const parent = findNode(root, parentPath);
    if (!parent?.children) continue;

    const oldName = path.split(/[\\/]/).pop()!;
    const stem = oldName.replace(/\.[^.]+$/, '');
    const ext = oldName.includes('.') ? oldName.substring(oldName.lastIndexOf('.')) : '';

    // 生成不重复的名称
    let copyName: string;
    let counter = 1;
    do {
      copyName = counter === 1
        ? `${stem} - Copy${ext}`
        : `${stem} - Copy ${counter}${ext}`;
      counter++;
    } while (parent.children[copyName]);

    const copyPath = `${parentPath}\\${copyName}`;
    parent.children[copyName] = {
      name: copyName,
      path: copyPath,
      is_dir: false,
      content: node.content || '',
    };
    saveFs(store);
    return { name: copyName, path: copyPath, is_dir: false, size: node.content?.length || 0 };
  }
  throw new Error(`Cannot duplicate: file not found: ${path}`);
}

/** 获取所有已保存的项目 */
export function mockGetProjects(): ProjectInfo[] {
  return loadFs().projects;
}

/** 获取项目文件树 */
export function mockGetProjectTree(projectPath: string): FileEntry[] {
  return mockListDirectory(projectPath);
}

/** 检查文件是否存在 */
export function mockFileExists(path: string): boolean {
  const roots = loadFs().roots;
  for (const rootPath of Object.keys(roots)) {
    const node = findNode(roots[rootPath], path);
    if (node) return true;
  }
  return false;
}

// ==================== 模板文件 ====================

interface TemplateFile {
  name: string;
  content: string;
}

function getTemplateFiles(projectName: string, template: string): TemplateFile[] {
  const files: TemplateFile[] = [
    {
      name: 'CMakeLists.txt',
      content: `cmake_minimum_required(VERSION 3.16)
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(${projectName})`,
    },
  ];

  switch (template) {
    case 'wifi':
      files.push({
        name: 'sdkconfig.defaults',
        content: `CONFIG_ESP_WIFI_SSID="YOUR_SSID"
CONFIG_ESP_WIFI_PASSWORD="YOUR_PASSWORD"`,
      });
      break;
    case 'ble':
      files.push({
        name: 'sdkconfig.defaults',
        content: 'CONFIG_BT_ENABLED=y\nCONFIG_BT_BLE_ENABLED=y',
      });
      break;
    case 'sensor':
      files.push({
        name: 'sdkconfig.defaults',
        content: '# I2C sensor configuration',
      });
      break;
  }

  return files;
}

function getMainTemplate(projectName: string): string {
  return `#include <stdio.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_system.h"
#include "esp_log.h"

static const char *TAG = "${projectName}";

void app_main(void)
{
    ESP_LOGI(TAG, "Hello from ${projectName}!");
    ESP_LOGI(TAG, "ESP-IDF version: %s", esp_get_idf_version());

    while (1) {
        ESP_LOGI(TAG, "Running...");
        vTaskDelay(pdMS_TO_TICKS(5000));
    }
}
`;
}