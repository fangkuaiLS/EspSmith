/**
 * 项目相关类型定义
 */

// 项目配置
export interface ProjectConfig {
  name: string;
  path: string;
  chip: string;
  idf_path: string;
}

// 项目信息
export interface ProjectInfo {
  name: string;
  path: string;
  chip: string;
  idf_version: string;
  has_hardware_config: boolean;
}

// 文件条目
export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}
