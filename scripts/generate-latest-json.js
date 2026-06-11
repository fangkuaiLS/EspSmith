#!/usr/bin/env node

/**
 * 从打包产物自动生成 Tauri updater 所需的 latest.json
 *
 * 扫描 src-tauri/target/release/bundle/msi/ 目录，
 * 读取 .msi 和 .msi.sig 文件，生成 latest.json。
 *
 * 使用: node scripts/generate-latest-json.js
 *
 * 生成的 latest.json 会放在 msi 目录下，打包发布时一起上传到 GitHub Release。
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.join(__dirname, '..');

function main() {
  const packageJsonPath = path.join(rootDir, 'package.json');
  const bundleDir = path.join(rootDir, 'src-tauri', 'target', 'release', 'bundle', 'msi');

  if (!fs.existsSync(bundleDir)) {
    console.error('[generate-latest-json] Bundle directory not found:', bundleDir);
    console.error('[generate-latest-json] Please run "npm run tauri build" first.');
    process.exit(1);
  }

  // 读取版本号
  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf-8'));
  const version = packageJson.version;

  // 扫描 msi 目录，只处理当前版本的 MSI
  const files = fs.readdirSync(bundleDir);
  const msiFiles = files.filter(f => f.endsWith('.msi') && !f.endsWith('.msi.sig') && f.includes(version));
  const sigFiles = files.filter(f => f.endsWith('.msi.sig') && f.includes(version));

  if (msiFiles.length === 0) {
    console.error('[generate-latest-json] No .msi files found in bundle directory.');
    process.exit(1);
  }

  console.log(`[generate-latest-json] Version: ${version}`);
  console.log(`[generate-latest-json] Found ${msiFiles.length} MSI file(s):`);

  // 构建 platforms 数据
  const platforms = {};

  for (const msiFile of msiFiles) {
    const sigFile = `${msiFile}.sig`;
    if (!sigFiles.includes(sigFile)) {
      console.warn(`[generate-latest-json] WARNING: No signature file found for ${msiFile}, skipping.`);
      continue;
    }

    const sigPath = path.join(bundleDir, sigFile);
    const signature = fs.readFileSync(sigPath, 'utf-8').trim();

    const url = `https://github.com/fangkuaiLS/EspSmith/releases/download/v${version}/${msiFile}`;

    // 默认 windows-x86_64（如果后续支持 arm64 可根据文件名判断）
    platforms['windows-x86_64'] = {
      signature,
      url,
    };

    console.log(`  - ${msiFile} + ${sigFile}`);
  }

  if (Object.keys(platforms).length === 0) {
    console.error('[generate-latest-json] No valid platform entries generated.');
    process.exit(1);
  }

  // 生成 latest.json
  const latestJson = {
    version,
    notes: '',  // 发布时手动填写，或者从环境变量/CI 传入
    pub_date: new Date().toISOString(),
    platforms,
  };

  const outputPath = path.join(bundleDir, 'latest.json');
  fs.writeFileSync(outputPath, JSON.stringify(latestJson, null, 2) + '\n', 'utf-8');

  console.log(`[generate-latest-json] Generated: ${outputPath}`);
  console.log('[generate-latest-json] Done! Upload this file to your GitHub Release assets.');
}

main();
