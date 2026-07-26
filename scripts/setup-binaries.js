import { existsSync, mkdirSync, createWriteStream, writeFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { get } from 'https';
import { platform } from 'os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(__dirname, '..');
const binariesDir = join(projectRoot, 'src-tauri', 'binaries');

// Binary name mapping based on platform
const BINARIES = (() => {
  const os = platform();
  if (os === 'win32') {
    return [
      'codewhale-windows-x64.exe',
      'codewhale-tui-windows-x64.exe',
    ];
  }
  // macOS / Linux: codewhale is npm-installed at runtime, no bundled binaries needed
  return [];
})();

const GITHUB_REPO = 'fangkuaiLS/EspSmith';
const GITHUB_API = `https://api.github.com/repos/${GITHUB_REPO}/releases`;

function ensureDir(dir) {
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
}

/**
 * Fetch JSON from a URL with redirect following.
 */
function fetchJson(url) {
  return new Promise((resolve, reject) => {
    const req = get(url, { headers: { 'User-Agent': 'EspSmith-setup-binaries' } }, (res) => {
      // Follow redirects
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        fetchJson(res.headers.location).then(resolve).catch(reject);
        return;
      }
      if (res.statusCode !== 200) {
        reject(new Error(`HTTP ${res.statusCode}`));
        return;
      }
      let data = '';
      res.on('data', (chunk) => { data += chunk; });
      res.on('end', () => {
        try {
          resolve(JSON.parse(data));
        } catch (e) {
          reject(e);
        }
      });
    });
    req.on('error', reject);
  });
}

/**
 * Download a file from URL to destPath.
 */
function downloadFile(url, destPath) {
  return new Promise((resolve, reject) => {
    const req = get(url, { headers: { 'User-Agent': 'EspSmith-setup-binaries' } }, (res) => {
      // Follow redirects
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        downloadFile(res.headers.location, destPath).then(resolve).catch(reject);
        return;
      }
      if (res.statusCode !== 200) {
        reject(new Error(`HTTP ${res.statusCode}`));
        return;
      }
      const file = createWriteStream(destPath);
      res.pipe(file);
      file.on('finish', () => {
        file.close();
        resolve();
      });
      file.on('error', reject);
    });
    req.on('error', reject);
  });
}

async function tryDownloadFromRelease(releaseUrl, fileName) {
  // Fetch release info to find the asset
  const release = await fetchJson(releaseUrl);
  if (!release || !Array.isArray(release.assets)) {
    throw new Error('Invalid release response');
  }
  const asset = release.assets.find(a => a.name === fileName);
  if (!asset) {
    throw new Error(`Asset "${fileName}" not found in release`);
  }
  console.log(`  Downloading ${fileName} (${(asset.size / 1024 / 1024).toFixed(1)} MB)...`);
  const destPath = join(binariesDir, fileName);
  await downloadFile(asset.browser_download_url, destPath);
  console.log(`  OK: ${destPath}`);
}

/**
 * Create empty placeholder files for CI compilation checks.
 * These are NOT valid binaries - they only satisfy Tauri's resource existence check.
 * The app will fall back to npm-installed codewhale at runtime.
 */
function createPlaceholders() {
  ensureDir(binariesDir);
  for (const name of BINARIES) {
    const dest = join(binariesDir, name);
    if (!existsSync(dest)) {
      console.log(`  Creating placeholder: ${name} (0 bytes)`);
      // Empty placeholder - only satisfies Tauri's resource existence check.
      // At runtime, the app falls back to npm-installed codewhale.
      writeFileSync(dest, '');
    }
  }
}

async function main() {
  if (BINARIES.length === 0) {
    console.log('setup-binaries: No bundled binaries needed for this platform.');
    return;
  }

  ensureDir(binariesDir);

  // Check which binaries are missing
  const missing = BINARIES.filter(name => !existsSync(join(binariesDir, name)));

  if (missing.length === 0) {
    console.log('setup-binaries: All binaries present, nothing to do.');
    return;
  }

  console.log(`setup-binaries: Missing binaries: ${missing.join(', ')}`);

  // Try downloading from the latest GitHub Release
  try {
    console.log('  Fetching latest release info...');
    await tryDownloadFromRelease(`${GITHUB_API}/latest`, missing[0]);
    // If one succeeded, try the rest
    for (let i = 1; i < missing.length; i++) {
      await tryDownloadFromRelease(`${GITHUB_API}/latest`, missing[i]);
    }
    return;
  } catch (e) {
    console.log(`  Download from latest release failed: ${e.message}`);
  }

  // Fallback: try older releases (iterate through last 5 releases)
  try {
    console.log('  Searching older releases...');
    const releases = await fetchJson(`${GITHUB_API}?per_page=5`);
    for (const release of releases) {
      const assets = release.assets || [];
      for (const name of missing) {
        if (assets.some(a => a.name === name) && !existsSync(join(binariesDir, name))) {
          try {
            await tryDownloadFromRelease(release.url, name);
          } catch (e) {
            console.log(`  Failed to download ${name} from ${release.tag_name}: ${e.message}`);
          }
        }
      }
    }
  } catch (e) {
    console.log(`  Older releases search failed: ${e.message}`);
  }

  // Final fallback: create placeholders (for CI checks)
  const stillMissing = BINARIES.filter(name => !existsSync(join(binariesDir, name)));
  if (stillMissing.length > 0) {
    console.log('  No release with binaries found, creating placeholders for CI.');
    createPlaceholders();
  }
}

main().catch((e) => {
  console.error(`setup-binaries: ERROR: ${e.message}`);
  // Don't fail the build - create placeholders if possible
  try {
    createPlaceholders();
  } catch {
    process.exit(1);
  }
});
