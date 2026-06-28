/**
 * Shared file icon mapping.
 *
 * Used by CodeEditor tabs and FileTree to avoid duplicate icon definitions.
 */
import React from 'react';
import {
  FileCode, FileJson, FileText, FileCog, Image, File,
} from 'lucide-react';

export const FILE_ICONS: Record<string, React.ComponentType<{ size?: number | string; className?: string }>> = {
  c: FileCode,
  h: FileCode,
  cpp: FileCode,
  hpp: FileCode,
  py: FileCode,
  json: FileJson,
  md: FileText,
  txt: FileText,
  toml: FileCog,
  yaml: FileCog,
  yml: FileCog,
  png: Image,
  jpg: Image,
  jpeg: Image,
  gif: Image,
  svg: Image,
};

/** Default icon for unknown file types. */
export const DEFAULT_FILE_ICON = File;

/** Get icon component by filename extension. */
export function getFileIcon(name: string): React.ComponentType<{ size?: number | string; className?: string }> {
  const ext = name.split('.').pop()?.toLowerCase() || '';
  return FILE_ICONS[ext] || File;
}
