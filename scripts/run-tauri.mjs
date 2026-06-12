import { spawn, execSync } from 'child_process';
import { join, dirname, resolve } from 'path';
import { fileURLToPath } from 'url';
import { existsSync, mkdirSync } from 'fs';
import { platform, homedir } from 'os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, '..');

function hasNonAscii(str) {
  if (!str) return false;
  return /[^\x00-\x7F]/.test(str);
}

function ensureDir(dir) {
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
}

function findInPath(cmd) {
  try {
    const result = execSync(`where ${cmd} 2>nul`, {
      encoding: 'utf-8',
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: 'cmd',
    });
    return result.trim().split('\n')[0] || null;
  } catch {
    return null;
  }
}

function hasRustupToolchain(toolchain) {
  try {
    const result = execSync('rustup toolchain list 2>nul', {
      encoding: 'utf-8',
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: 'cmd',
    });
    return result.includes(toolchain);
  } catch {
    return false;
  }
}

function detectAndSetupToolchain() {
  if (platform() !== 'win32') return;

  const hasLink = findInPath('link.exe');
  if (hasLink) {
    return;
  }

  if (!hasRustupToolchain('stable-x86_64-pc-windows-gnu')) {
    process.stderr.write('[esp-ai-studio] MSVC tools not found, installing GNU toolchain...\n');
    try {
      execSync('rustup toolchain install stable-x86_64-pc-windows-gnu', {
        stdio: 'inherit',
        shell: 'cmd',
      });
    } catch {
      process.stderr.write(
        '[esp-ai-studio] ERROR: MSVC Build Tools not found and GNU toolchain install failed.\n' +
        '  Please install one of:\n' +
        '  1. Visual Studio Build Tools: https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022\n' +
        '  2. GNU toolchain manually: rustup toolchain install stable-x86_64-pc-windows-gnu\n'
      );
      process.exit(1);
    }
  }

  process.env.RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu';
  process.stderr.write('[esp-ai-studio] Using GNU toolchain (MSVC Build Tools not detected)\n');
}

function setupEnv() {
  const warnings = [];
  const os = platform();
  const home = homedir() || '';

  if (os === 'win32') {
    const localCargoHome = join(projectRoot, '.cargo-home');
    const systemCargoHome = process.env.CARGO_HOME || join(home, '.cargo');

    if (hasNonAscii(systemCargoHome)) {
      ensureDir(localCargoHome);
      process.env.CARGO_HOME = localCargoHome;
      warnings.push(
        `CARGO_HOME contains non-ASCII characters, redirected: "${systemCargoHome}" -> "${localCargoHome}"`
      );
    }

    const tempDir = process.env.TEMP || process.env.TMP || '';
    if (hasNonAscii(tempDir)) {
      const localTemp = join(projectRoot, '.tmp');
      ensureDir(localTemp);
      process.env.TEMP = localTemp;
      process.env.TMP = localTemp;
      warnings.push(
        `TEMP contains non-ASCII characters, redirected: "${tempDir}" -> "${localTemp}"`
      );
    }

    if (hasNonAscii(home)) {
      warnings.push(
        `Home directory "${home}" contains non-ASCII characters. ` +
        `This may cause issues with some tools. If you encounter problems, ` +
        `consider moving the project to a path without Chinese characters ` +
        `(e.g., C:\\Projects\\esp-ai-studio).`
      );
    }

    if (hasNonAscii(projectRoot)) {
      warnings.push(
        `Project path "${projectRoot}" contains non-ASCII characters. ` +
        `This is known to cause Rust build script failures on Chinese Windows. ` +
        `Please move the project to a pure ASCII path (e.g., C:\\Projects\\esp-ai-studio).`
      );
    }
  }

  return warnings;
}

detectAndSetupToolchain();

const mingwBin = join(projectRoot, 'tools', 'mingw64', 'bin');
if (existsSync(mingwBin)) {
  process.env.PATH = `${mingwBin};${process.env.PATH}`;
}

const warnings = setupEnv();
if (warnings.length > 0) {
  const separator = '─'.repeat(72);
  process.stderr.write(`\n${separator}\n`);
  process.stderr.write(`  [esp-ai-studio] Environment Warnings\n`);
  process.stderr.write(`${separator}\n`);
  for (const w of warnings) {
    process.stderr.write(`  ⚠  ${w}\n`);
  }
  process.stderr.write(`${separator}\n\n`);
}

const args = process.argv.slice(2);

// 如果是 dev 命令，确保 espsmith-cli.exe 已编译
// cargo tauri dev 只编译 espsmith.exe（GUI），不编译 espsmith-cli.exe（console）。
// build.rs 已用 catch_unwind 包裹 tauri_build::build()，所以 cargo build --bin espsmith-cli
// 不会再 panic。这里在 tauri dev 启动前先编译 CLI binary。
if (args.includes('dev')) {
  const cliExePath = join(projectRoot, 'src-tauri', 'target', 'debug', 'espsmith-cli.exe');
  // 检查是否需要重新编译：文件不存在，或者 lib.rs 比 cli.exe 更新
  const needsCompile = !existsSync(cliExePath) || (() => {
    const { statSync } = require('fs');
    try {
      const cliTime = statSync(cliExePath).mtimeMs;
      const libTime = statSync(join(projectRoot, 'src-tauri', 'src', 'lib.rs')).mtimeMs;
      return libTime > cliTime;
    } catch { return true; }
  })();
  if (needsCompile) {
    process.stderr.write('[esp-ai-studio] Compiling espsmith-cli.exe for dev mode...\n');
    try {
      execSync('cargo build --bin espsmith-cli --manifest-path src-tauri/Cargo.toml', {
        stdio: 'inherit',
        cwd: projectRoot,
        shell: true,
      });
      process.stderr.write('[esp-ai-studio] espsmith-cli.exe compiled successfully.\n');
    } catch (error) {
      process.stderr.write(`[esp-ai-studio] WARNING: Failed to compile espsmith-cli.exe: ${error.message}\n`);
      process.stderr.write('[esp-ai-studio] Falling back to espsmith.exe for CLI operations (may not capture output).\n');
    }
  }
}

// 如果是 build 命令，先同步版本号
if (args.includes('build')) {
  process.stderr.write('[esp-ai-studio] Syncing version numbers...\n');
  try {
    execSync('npm run sync-version', {
      stdio: 'inherit',
      cwd: projectRoot,
      shell: true,
    });
  } catch (error) {
    process.stderr.write(`[esp-ai-studio] WARNING: Failed to sync version: ${error.message}\n`);
  }
}

const child = spawn('npx', ['tauri', ...args], {
  stdio: 'inherit',
  env: process.env,
  shell: true,
});

child.on('close', (code) => {
  // build 成功后自动生成 latest.json
  if (code === 0 && args.includes('build')) {
    process.stderr.write('[esp-ai-studio] Generating latest.json for updater...\n');
    try {
      execSync('node scripts/generate-latest-json.js', {
        stdio: 'inherit',
        cwd: projectRoot,
        shell: true,
      });
    } catch (error) {
      process.stderr.write(`[esp-ai-studio] WARNING: Failed to generate latest.json: ${error.message}\n`);
    }
  }
  process.exit(code);
});