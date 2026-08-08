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
  /** AI 推理/思考过程内容（独立于正式回复，可折叠展示） */
  thinkingContent?: string;
  /** 标记为工具调用间的过程叙述（淡色步骤流渲染，非正文回复） */
  isNarration?: boolean;
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


