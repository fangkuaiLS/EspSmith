/**
 * 聊天相关类型定义
 */

// 聊天消息
export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: number;
  status?: 'sending' | 'sent' | 'error';
  toolData?: { name: string; input?: unknown; result?: string };
  usage?: { inputTokens: number; outputTokens: number; cachedTokens: number; totalTokens: number; costRmb: number };
}

// AI 状态
export type AIStatus = 'idle' | 'thinking' | 'building' | 'flashing' | 'error';

// AI 用量统计
export interface AIUsage {
  inputTokens: number;
  outputTokens: number;
  cachedTokens: number;
  totalTokens: number;
  costRmb: number;
  model: string;
}

export interface AICumulativeUsage {
  session: AIUsage;
  lastMessage: AIUsage;
  messageCount: number;
}


