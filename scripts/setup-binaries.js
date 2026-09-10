import { existsSync, mkdirSync, createWriteStream, statSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { get } from 'https';
import { platform } from 'os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(__dirname, '..');
const binariesDir = join(projectRoot, 'src-tauri', 'binaries');

// Binary name mapping based on platform
const BINARIES = (() => {
  if (platform() === 'win32') {
    return [
      'codewhale-windows-x64.exe',
      'codewhale-tui-windows-x64.exe',
    ];
  }
  // macOS / Linux: codewhale is npm-installed at runtime, no bundled binaries needed
  return [];
})();

// Release metadata sources, tried in order.
// 1. Upstream CodeWhale releases - authoritative source of real binaries.
// 2. EspSmith own releases - optional mirror in case binaries are uploaded there.
const RELEASE_URLS = [
  'https://api.github.com/repos/Hmbown/CodeWhale/releases/latest',
  'https://api.github.com/repos/fangkuaiLS/EspSmith/releases/latest',
];

// Download URL mirrors, tried in order. Same list as the updater endpoints
// in tauri.conf.json (trusted proxies for GitHub release downloads).
const DOWNLOAD_MIRRORS = ['', 'https://ghproxy.net/', 'https://gh-proxy.com/', 'https://mirror.ghproxy.com/'];

const DOWNLOAD_RETRIES = 2;

// Abort a download if no bytes flow for this long (stalled connection).
const IDLE_TIMEOUT_MS = 30000;

function ensureDir(dir) {
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
}

function fileSize(path) {
  try {
    return statSync(path).size;
  } catch {
    return -1;
  }
}

/**
 * Fetch JSON from a URL with redirect following.
 */
function fetchJson(url) {
  return new Promise((resolve, reject) => {
    const req = get(url, { headers: { 'User-Agent': 'EspSmith-setup-binaries' } }, (res) => {
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
    req.setTimeout(IDLE_TIMEOUT_MS, () => {
      req.destroy(new Error(`stalled: no data for ${IDLE_TIMEOUT_MS / 1000}s`));
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
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        downloadFile(res.headers.location, destPath).then(resolve).catch(reject);
        return;
      }
      if (res.statusCode !== 200) {
        reject(new Error(`HTTP ${res.statusCode}`));
        return;
      }
      res.on('error', reject);
      const file = createWriteStream(destPath);
      res.pipe(file);
      file.on('finish', () => {
        file.close();
        resolve();
      });
      file.on('error', reject);
    });
    // Covers connect, response-wait, and body-transfer stalls on one socket.
    req.setTimeout(IDLE_TIMEOUT_MS, () => {
      req.destroy(new Error(`stalled: no data for ${IDLE_TIMEOUT_MS / 1000}s`));
    });
    req.on('error', reject);
  });
}

/**
 * Fetch the first reachable release metadata.
 */
async function fetchRelease() {
  for (const url of RELEASE_URLS) {
    try {
      const release = await fetchJson(url);
      console.log(`setup-binaries: Using release ${release.tag_name} (${url})`);
      return release;
    } catch (e) {
      console.log(`  Release source failed: ${e.message}`);
    }
  }
  return null;
}

/**
 * Download one asset via every mirror, with retries and size verification.
 */
async function downloadAsset(asset, destPath) {
  for (let attempt = 0; attempt <= DOWNLOAD_RETRIES; attempt++) {
    for (const mirror of DOWNLOAD_MIRRORS) {
      if (mirror) console.log(`  Trying mirror ${mirror}...`);
      try {
        await downloadFile(mirror + asset.browser_download_url, destPath);
        const actual = fileSize(destPath);
        if (actual !== asset.size) {
          throw new Error(`truncated download: got ${actual} bytes, expected ${asset.size}`);
        }
        console.log(`  OK: ${destPath}`);
        return true;
      } catch (e) {
        console.log(`  Download failed: ${e.message}`);
      }
    }
    if (attempt < DOWNLOAD_RETRIES) {
      console.log(`  Retrying (attempt ${attempt + 2}/${DOWNLOAD_RETRIES + 1})...`);
      await new Promise(r => setTimeout(r, 2000));
    }
  }
  return false;
}

async function main() {
  if (BINARIES.length === 0) {
    console.log('setup-binaries: No bundled binaries needed for this platform.');
    return;
  }

  ensureDir(binariesDir);

  const release = await fetchRelease();
  if (!release) {
    console.error('setup-binaries: FATAL: no release source reachable.');
    process.exit(1);
  }
  const assets = release.assets || [];

  for (const name of BINARIES) {
    const asset = assets.find(a => a.name === name);
    if (!asset) {
      console.error(`setup-binaries: FATAL: asset "${name}" not found in release ${release.tag_name}.`);
      process.exit(1);
    }

    // Exact size match decides whether an existing file is valid.
    // Truncated leftovers from interrupted runs are re-downloaded.
    const destPath = join(binariesDir, name);
    if (fileSize(destPath) === asset.size) {
      console.log(`  OK (already present): ${name}`);
      continue;
    }

    console.log(`  Downloading ${name} (${(asset.size / 1024 / 1024).toFixed(1)} MB)...`);
    const ok = await downloadAsset(asset, destPath);
    if (!ok) {
      console.error(`setup-binaries: FATAL: could not download "${name}" after all retries and mirrors.`);
      console.error('  Refusing to continue: a truncated binary would silently break the bundled app.');
      process.exit(1);
    }
  }

  console.log('setup-binaries: Done.');
}

main().catch((e) => {
  console.error(`setup-binaries: ERROR: ${e.message}`);
  process.exit(1);
});
