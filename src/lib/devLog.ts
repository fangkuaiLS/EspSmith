/**
 * Development-only logging utility.
 *
 * In production builds, these calls become no-ops (tree-shaken by the bundler
 * when `import.meta.env.DEV` is `false`).
 *
 * Usage:
 *   import { devLog } from '../lib/devLog';
 *   devLog('[Component]', 'debug info', data);
 */

type LogArgs = unknown[];

/** Log only in development mode. No-op in production. */
export function devLog(...args: LogArgs): void {
  if (import.meta.env.DEV) {
    console.log(...args);
  }
}

/** Warn only in development mode. No-op in production. */
export function devWarn(...args: LogArgs): void {
  if (import.meta.env.DEV) {
    console.warn(...args);
  }
}

/** Error logging — always active, even in production. */
export function devError(...args: LogArgs): void {
  console.error(...args);
}
