/**
 * 编译与调试相关类型定义
 */

export interface CompileError {
  file: string;
  line: number;
  column: number;
  error_type: 'error' | 'warning';
  message: string;
}

export interface BuildResult {
  success: boolean;
  output: string;
  errors: CompileError[];
}

export interface FlashResult {
  success: boolean;
  output: string;
}

export interface DebugState {
  running: boolean;
  pc: string;
  stack: StackFrame[];
  locals: [string, string][];
  registers: [string, string][];
}

export interface StackFrame {
  level: number;
  function: string;
  file: string;
  line: number;
  address: string;
}

export interface Breakpoint {
  id: number;
  file: string;
  line: number;
  address: string;
  enabled: boolean;
  hit_count: number;
}

export interface VariableInfo {
  name: string;
  value: string;
  type_name: string;
}

export interface SerialPortInfo {
  name: string;
  path: string;
  port_name?: string;
  vid?: string;
  pid?: string;
  chip_type?: string;
}

export interface ChipTargetInfo {
  target: string;
  label: string;
  is_preview: boolean;
  description: string;
}