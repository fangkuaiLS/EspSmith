#!/usr/bin/env node

/**
 * 版本号统一管理脚本
 *
 * 从 package.json 读取版本号，并同步更新到所有相关文件
 * 使用: node scripts/sync-version.js
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.join(__dirname, '..');
const packageJsonPath = path.join(rootDir, 'package.json');

// 读取 package.json 中的版本号
const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf-8'));
const version = packageJson.version;

console.log(`📦 Current version: ${version}`);

// 需要更新的文件列表
const filesToUpdate = [
  {
    path: path.join(rootDir, 'src-tauri', 'tauri.conf.json'),
    pattern: /"version":\s*"[^"]+"/g,
    replacement: `"version": "${version}"`,
  },
  {
    path: path.join(rootDir, 'src-tauri', 'Cargo.toml'),
    pattern: /^version = "[^"]+"/gm,
    replacement: `version = "${version}"`,
  },
  {
    path: path.join(rootDir, 'src', 'i18n', 'locales', 'en.json'),
    pattern: /"version":\s*"v[^"]+"/g,
    replacement: `"version": "v${version}"`,
  },
  {
    path: path.join(rootDir, 'src', 'i18n', 'locales', 'zh.json'),
    pattern: /"version":\s*"v[^"]+"/g,
    replacement: `"version": "v${version}"`,
  },
  {
    path: path.join(rootDir, 'src', 'components', 'settings', 'SettingsDialog.tsx'),
    pattern: /v\d+\.\d+\.\d+/g,
    replacement: `v${version}`,
  },
  {
    path: path.join(rootDir, 'src-tauri', 'src', 'mcp.rs'),
    pattern: /"version":\s*"[^"]+"/g,
    replacement: `"version": "${version}"`,
  },
  {
    path: path.join(rootDir, 'README.md'),
    pattern: /v\d+\.\d+\.\d+/g,
    replacement: `v${version}`,
  },
  {
    path: path.join(rootDir, 'README_EN.md'),
    pattern: /v\d+\.\d+\.\d+/g,
    replacement: `v${version}`,
  },
];

// 更新文件
let updatedCount = 0;
filesToUpdate.forEach(({ path: filePath, pattern, replacement }) => {
  try {
    const content = fs.readFileSync(filePath, 'utf-8');
    const newContent = content.replace(pattern, replacement);

    if (content !== newContent) {
      fs.writeFileSync(filePath, newContent, 'utf-8');
      console.log(`✅ Updated: ${path.relative(rootDir, filePath)}`);
      updatedCount++;
    } else {
      console.log(`⏭️  Skipped: ${path.relative(rootDir, filePath)} (already up to date)`);
    }
  } catch (error) {
    console.error(`❌ Failed to update ${path.relative(rootDir, filePath)}:`, error.message);
  }
});

console.log(`\n🎉 Version sync complete! Updated ${updatedCount} files.`);