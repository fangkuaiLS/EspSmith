import { spawn, execSync } from 'child_process';
import { join, dirname, resolve } from 'path';
import { fileURLToPath } from 'url';
import { existsSync, mkdirSync, statSync, writeFileSync, unlinkSync } from 'fs';
import { platform, homedir } from 'os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, '..');

// 从 npm_package_version 注入版本号给 Tauri
process.env.TAURI_APP_VERSION = process.env.npm_package_version;

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

function getCargoTargetDir() {
  // 通过 cargo metadata 获取实际解析后的 target 目录（尊重 .cargo/config.toml 的 target-dir）。
  // 这样无论配置指向何处，CLI 可执行文件路径都能正确匹配，避免因路径不一致导致每次 dev 都重复编译 CLI。
  try {
    const out = execSync(
      'cargo metadata --format-version 1 --no-deps --manifest-path src-tauri/Cargo.toml',
      { encoding: 'utf-8', stdio: ['ignore', 'pipe', 'ignore'], cwd: projectRoot, shell: true }
    );
    return JSON.parse(out).target_directory;
  } catch {
    // 极端情况下 cargo metadata 失败时回退到默认位置
    return join(projectRoot, 'src-tauri', 'target');
  }
}

// 检测 MSVC 链接器是否可用。
// 关键：MSVC 的 link.exe 正常情况下不在 PATH（它由 vcvarsall 注入开发者命令行环境），
// 因此不能用 `where link.exe` 检测——会误判为缺失并错误地切到 GNU 工具链。
// rustc 自身通过 vswhere + 注册表 + vcvarsall 发现 MSVC，与正式编译完全一致；
// 这里让 rustc 实际编译+链接一个最小程序作为“能否链接二进制”的权威判定。
// （已验证：本机 VS 2022 Preview 不被 `vswhere`（不带 -prerelease）列出，也不在 PATH，
//  但 rustc 能正确发现其 link.exe。故只有 rustc 探针可靠。）
function msvcLinkerAvailable() {
  if (platform() !== 'win32') return false;
  // 快速正路径：link.exe 已在 PATH（开发者命令行环境），无需探针。
  if (findInPath('link.exe')) return true;
  // 权威判定：让 rustc 按 rust-toolchain.toml 指定的工具链（MSVC host）尝试链接最小程序。
  const probeDir = join(projectRoot, '.tmp');
  const probeSrc = join(probeDir, 'msvc_probe.rs');
  const probeExe = join(probeDir, 'msvc_probe.exe');
  ensureDir(probeDir);
  try {
    writeFileSync(probeSrc, 'fn main(){}\n');
    execSync(`rustc "${probeSrc}" -o "${probeExe}" --crate-type bin`, {
      stdio: 'ignore',
      shell: true,
      cwd: projectRoot,
    });
    return existsSync(probeExe);
  } catch {
    return false;
  } finally {
    try { unlinkSync(probeExe); } catch {}
    try { unlinkSync(probeSrc); } catch {}
    // 顺带清理 rustc 可能产生的 .pdb
    try { unlinkSync(join(probeDir, 'msvc_probe.pdb')); } catch {}
  }
}

function detectAndSetupToolchain() {
  if (platform() !== 'win32') return;

  // MSVC 可用则遵从 rust-toolchain.toml，不覆盖工具链。
  if (msvcLinkerAvailable()) {
    return;
  }

  // MSVC 不可用才回退 GNU。
  // 注意：GNU 工具链链接仍需系统装有 MinGW（gcc/ld/dlltool），
  // 否则会在链接 windows-sys 时报 "dlltool.exe: program not found"。
  // 故推荐优先安装 Visual Studio Build Tools（含 MSVC）。
  process.stderr.write(
    '[esp-ai-studio] MSVC linker not detected (rustc probe failed). Falling back to GNU toolchain.\n' +
    '  Note: GNU toolchain also requires MinGW (gcc/ld/dlltool) for linking; otherwise\n' +
    '  windows-sys will fail with "dlltool.exe: program not found".\n' +
    '  Recommended: install Visual Studio Build Tools (C++ workload):\n' +
    '    https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022\n'
  );

  if (!hasRustupToolchain('stable-x86_64-pc-windows-gnu')) {
    process.stderr.write('[esp-ai-studio] Installing GNU toolchain...\n');
    try {
      execSync('rustup toolchain install stable-x86_64-pc-windows-gnu', {
        stdio: 'inherit',
        shell: 'cmd',
      });
    } catch {
      process.stderr.write(
        '[esp-ai-studio] ERROR: MSVC not found and GNU toolchain install failed.\n' +
        '  Please install Visual Studio Build Tools (C++ workload) or MinGW manually.\n'
      );
      process.exit(1);
    }
  }

  process.env.RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu';
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
  // 加到 PATH 末尾，避免 mingw 的 ld.exe 干扰 Rust MSVC target 的 link.exe 查找。
  // Rust MSVC target 通过 vswhere 定位 Visual Studio 的 link.exe，不依赖 PATH，
  // 但 mingw 的 ld.exe 若在 PATH 开头会导致链接器符号解析异常（如 core::fmt::Arguments::from_str）。
  process.env.PATH = `${process.env.PATH};${mingwBin}`;
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
  const cliExePath = join(getCargoTargetDir(), 'debug', 'espsmith-cli.exe');
  // 检查是否需要重新编译：文件不存在，或者 lib.rs 比 cli.exe 更新
  const needsCompile = !existsSync(cliExePath) || (() => {
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