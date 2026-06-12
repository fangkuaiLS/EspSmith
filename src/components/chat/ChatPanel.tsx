/**
 * ChatPanel - AI 聊天面板组件 (Codex-inspired)
 *
 * 功能：
 * - 消息列表显示（Markdown 渲染）
 * - 消息输入框（支持 Enter 发送）
 * - AI 状态指示（脉动圆点）
 * - 快捷命令
 */

import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { translateBackendString } from '../../i18n';
import { Bot, Send, StopCircle, Plus, User, Code, Terminal, ChevronDown, ChevronRight, ExternalLink, Undo2, Coins, Copy, Check, Pencil, Clock, Trash2, Shield, ShieldAlert, Cpu, Loader, CheckCircle2, Circle, XCircle } from 'lucide-react';
import { useChatStore, useSettingsStore } from '../../stores';
import { useProjectStore } from '../../stores/projectStore';
import type { ChatSession } from '../../stores/chatStore';
import { useFileStore } from '../../stores/fileStore';
import { AIStatus, ChatMessage, OperationProgress } from '../../stores/chatStore';
import type { AICumulativeUsage } from '../../types/chat';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkBreaks from 'remark-breaks';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { PermissionModal } from './PermissionModal';

// 解析工具输入中的文件路径，支持相对路径转绝对路径
function resolveFilePath(input: Record<string, unknown> | undefined): string | null {
  if (!input) return null;
  const rawPath = (input.path || input.file_path || input.filePath) as string | undefined;
  if (!rawPath) return null;
  // 绝对路径直接返回
  if (/^[A-Z]:[\\/]/i.test(rawPath) || rawPath.startsWith('/')) return rawPath;
  // 相对路径基于项目目录解析
  const project = useProjectStore.getState().currentProject;
  if (!project?.path) return rawPath;
  return `${project.path}\\${rawPath}`;
}

const STATUS_CONFIG: Record<AIStatus, { labelKey: string; dotClass: string }> = {
  idle: { labelKey: 'chat.ready', dotClass: 'bg-success' },
  thinking: { labelKey: 'chat.thinking', dotClass: 'bg-warning animate-pulse-dot' },
  building: { labelKey: 'chat.building', dotClass: 'bg-info animate-pulse-dot' },
  flashing: { labelKey: 'chat.flashing', dotClass: 'bg-warning animate-pulse-dot' },
  tool_call: { labelKey: 'chat.toolCall', dotClass: 'bg-accent animate-pulse-dot' },
  error: { labelKey: 'chat.error', dotClass: 'bg-error' },
};



interface ModelOption {
  id: string;
  label: string;
  provider: 'deepseek' | 'ollama' | 'mimo';
  model: string;
}

const MODEL_OPTIONS: ModelOption[] = [
  { id: 'deepseek-v4-pro', label: 'DeepSeek V4 Pro', provider: 'deepseek', model: 'deepseek-v4-pro' },
  { id: 'deepseek-v4-flash', label: 'DeepSeek V4 Flash', provider: 'deepseek', model: 'deepseek-v4-flash' },
  { id: 'ollama', label: 'Ollama (Local)', provider: 'ollama', model: 'ollama' },
  { id: 'mimo', label: 'MiMo-Code', provider: 'mimo', model: 'mimo' },
];

function getCurrentModelId(): string {
  const s = useSettingsStore.getState().settings;
  if (s.aiModel === 'ollama') return 'ollama';
  if (s.aiModel === 'mimo') return 'mimo';
  return s.deepseekModel || 'deepseek-v4-pro';
}

function formatTime(ts: number): string {
  const date = new Date(ts);
  const now = new Date();
  const diff = now.getTime() - date.getTime();
  if (diff < 60000) return '刚刚';
  if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`;
  if (date.getFullYear() === now.getFullYear()) {
    return `${date.getMonth() + 1}/${date.getDate()} ${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
  }
  return `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()}`;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function UsageBar({ usage }: { usage: AICumulativeUsage }) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2 px-3 py-1.5 bg-surface-root border-t border-border-subtle text-[10px] text-text-tertiary">
      <Coins size={11} className="text-warning" />
      <span title={t('chat.sessionTokens')}>
        {formatTokens(usage.session.totalTokens)} tokens
      </span>
      {usage.lastMessage.totalTokens > 0 && (
        <>
          <span className="text-text-disabled">|</span>
          <span title={t('chat.lastMsgTokens')}>
            ↑{formatTokens(usage.lastMessage.inputTokens)} ↓{formatTokens(usage.lastMessage.outputTokens)}
          </span>
          {usage.lastMessage.cachedTokens > 0 && (
            <span className="text-green-400" title="缓存命中 tokens">
              ⊞{formatTokens(usage.lastMessage.cachedTokens)}
            </span>
          )}
        </>
      )}
    </div>
  );
}

export function ChatPanel() {
  const { t } = useTranslation();
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [modelOpen, setModelOpen] = useState(false);
  const modelDropdownRef = useRef<HTMLDivElement>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const historyDropdownRef = useRef<HTMLDivElement>(null);
  const [permOpen, setPermOpen] = useState(false);
  const permDropdownRef = useRef<HTMLDivElement>(null);

  const inputHistoryRef = useRef<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const draftInputRef = useRef('');

  const { messages, status, pendingRollback, usage, sessions, permissionMode, pendingPermission, activeOperation, sendMessage, startAI, stopAI, clearMessages, confirmRollback, cancelRollback, loadSessions, loadSession, deleteSession, setPermissionMode } = useChatStore();
  const { settings, setSettings } = useSettingsStore();
  const projectPath = useProjectStore((s) => s.currentProject?.path);
  const currentModelId = getCurrentModelId();

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, [input]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    async function setupPermissionListener() {
      if (typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)) {
        try {
          const { listen } = await import('@tauri-apps/api/event');
          unlisten = await listen<{ toolName: string; reason: string }>('ai-permission-request', (event) => {
            useChatStore.setState({ pendingPermission: event.payload });
          });
        } catch { /* ignore */ }
      }
    }
    setupPermissionListener();
    return () => { unlisten?.(); };
  }, []);

  useEffect(() => {
    loadSessions();
    startAI();
  }, [projectPath]);

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (modelDropdownRef.current && !modelDropdownRef.current.contains(e.target as Node)) {
        setModelOpen(false);
      }
      if (historyDropdownRef.current && !historyDropdownRef.current.contains(e.target as Node)) {
        setHistoryOpen(false);
      }
      if (permDropdownRef.current && !permDropdownRef.current.contains(e.target as Node)) {
        setPermOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  useEffect(() => {
    function handleGlobalKeyDown(e: KeyboardEvent) {
      if (e.key === 'l' && e.ctrlKey && !e.shiftKey && !e.metaKey) {
        e.preventDefault();
        stopAI();
        clearMessages(true);
        loadSessions();
        startAI();
      }
    }
    window.addEventListener('keydown', handleGlobalKeyDown);
    return () => window.removeEventListener('keydown', handleGlobalKeyDown);
  }, [stopAI, clearMessages, startAI, loadSessions]);

  const handleModelChange = useCallback(async (option: ModelOption) => {
    setModelOpen(false);
    if (option.id === currentModelId) return;
    const newSettings = { ...settings };
    if (option.provider === 'deepseek') {
      newSettings.aiModel = 'deepseek';
      newSettings.deepseekModel = option.model as 'deepseek-v4-pro' | 'deepseek-v4-flash';
    } else if (option.provider === 'mimo') {
      newSettings.aiModel = 'mimo';
    } else {
      newSettings.aiModel = 'ollama';
    }
    setSettings(newSettings);
    stopAI();
    clearMessages(true);
    loadSessions();
    setTimeout(() => { startAI(); }, 150);
  }, [settings, setSettings, stopAI, clearMessages, startAI, currentModelId, loadSessions]);

  const handleNewTask = () => {
    stopAI();
    clearMessages(true);
    loadSessions();
    startAI();
  };

  const handleLoadSession = (session: ChatSession) => {
    setHistoryOpen(false);
    loadSession(session.id);
  };

  const handleDeleteSession = (e: React.MouseEvent, sessionId: string) => {
    e.stopPropagation();
    deleteSession(sessionId);
  };

  const handleSend = async () => {
    if (!input.trim()) return;
    inputHistoryRef.current.push(input.trim());
    setHistoryIndex(-1);
    draftInputRef.current = '';
    await sendMessage(input.trim());
    setInput('');
    textareaRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
      return;
    }
    if (e.key === 'ArrowUp' && !input) {
      e.preventDefault();
      const history = inputHistoryRef.current;
      if (history.length === 0) return;
      if (historyIndex === -1) {
        draftInputRef.current = '';
        const newIdx = history.length - 1;
        setHistoryIndex(newIdx);
        setInput(history[newIdx]);
      } else if (historyIndex > 0) {
        const newIdx = historyIndex - 1;
        setHistoryIndex(newIdx);
        setInput(history[newIdx]);
      }
      return;
    }
    if (e.key === 'ArrowDown') {
      const history = inputHistoryRef.current;
      if (historyIndex === -1) return;
      e.preventDefault();
      if (historyIndex < history.length - 1) {
        const newIdx = historyIndex + 1;
        setHistoryIndex(newIdx);
        setInput(history[newIdx]);
      } else {
        setHistoryIndex(-1);
        setInput(draftInputRef.current);
        draftInputRef.current = '';
      }
      return;
    }
    if (e.key === 'ArrowUp' && historyIndex === -1 && input) {
      draftInputRef.current = input;
    }
  };

  const statusConfig = STATUS_CONFIG[status];
  const isBusy = status === 'thinking' || status === 'building' || status === 'flashing' || status === 'tool_call';
  const lastAssistantMsg = messages[messages.length - 1];
  const showTyping = isBusy && (!lastAssistantMsg || lastAssistantMsg.role !== 'assistant' || !lastAssistantMsg.content);

  const groupedMessages = useMemo(() => {
    const groups: Array<{ type: 'single'; message: ChatMessage } | { type: 'tool_group'; messages: ChatMessage[] }> = [];
    let i = 0;
    while (i < messages.length) {
      if (messages[i].toolData) {
        const toolGroup: ChatMessage[] = [];
        while (i < messages.length && messages[i].toolData) {
          toolGroup.push(messages[i]);
          i++;
        }
        groups.push({ type: 'tool_group', messages: toolGroup });
      } else {
        groups.push({ type: 'single', message: messages[i] });
        i++;
      }
    }
    return groups;
  }, [messages]);

  return (
    <div className="h-full flex flex-col bg-surface-base">
      {pendingPermission && <PermissionModal />}
      {/* Header */}
      <div className="px-4 py-3 border-b border-border-default flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-lg bg-accent-muted flex items-center justify-center">
            <Bot size={17} className="text-accent" />
          </div>
          <div>
            <h3 className="text-[13px] font-semibold">{t('chat.aiAssistant')}</h3>
            <div className="flex items-center gap-1.5 mt-0.5">
              <div className={`w-1.5 h-1.5 rounded-full ${statusConfig.dotClass}`} />
              <span className="text-[11px] text-text-tertiary">{t(statusConfig.labelKey)}</span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <div className="relative" ref={modelDropdownRef}>
            <button
              onClick={() => setModelOpen(!modelOpen)}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-surface-overlay border border-border-subtle text-[11px] text-text-secondary hover:text-text-primary hover:border-border-default transition-all"
              title={t('chat.switchModel')}
            >
              <span className="max-w-[100px] truncate">
                {MODEL_OPTIONS.find(o => o.id === currentModelId)?.label || currentModelId}
              </span>
              <ChevronDown size={11} className={`transition-transform ${modelOpen ? 'rotate-180' : ''}`} />
            </button>
            {modelOpen && (
              <div className="absolute right-0 top-full mt-1 w-[200px] bg-surface-elevated border border-border-default rounded-lg shadow-lg z-50 py-1 animate-scale-in origin-top-right">
                {MODEL_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    onClick={() => handleModelChange(option)}
                    className={`w-full flex items-center gap-2 px-3 py-2 text-[12px] transition-colors ${
                      option.id === currentModelId
                        ? 'bg-accent-muted text-accent'
                        : 'text-text-secondary hover:text-text-primary hover:bg-surface-hover'
                    }`}
                  >
                    <div className={`w-2 h-2 rounded-full ${option.id === currentModelId ? 'bg-accent' : 'bg-text-disabled'}`} />
                    <span>{option.label}</span>
                    {option.id === currentModelId && <Check size={12} className="ml-auto text-accent" />}
                  </button>
                ))}
              </div>
            )}
          </div>
          <div className="relative" ref={historyDropdownRef}>
            <button
              onClick={() => { setHistoryOpen(!historyOpen); loadSessions(); }}
              className={`p-1.5 rounded-md transition-all ${
                historyOpen
                  ? 'bg-accent-muted text-accent'
                  : 'text-text-tertiary hover:text-text-primary hover:bg-surface-hover'
              }`}
              title="历史会话"
            >
              <Clock size={16} />
            </button>
            {historyOpen && (
              <div className="absolute right-0 top-full mt-1 w-[280px] bg-surface-elevated border border-border-default rounded-lg shadow-lg z-50 py-1 animate-scale-in origin-top-right max-h-[400px] overflow-y-auto">
                {sessions.length === 0 ? (
                  <div className="px-4 py-6 text-center text-[12px] text-text-tertiary">
                    暂无历史会话
                  </div>
                ) : (
                  sessions.map((session) => (
                    <div
                      key={session.id}
                      role="button"
                      tabIndex={0}
                      onClick={() => handleLoadSession(session)}
                      onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') handleLoadSession(session); }}
                      className="w-full flex items-center gap-3 px-3 py-2.5 text-left transition-colors hover:bg-surface-hover group cursor-pointer"
                    >
                      <div className="flex-1 min-w-0">
                        <div className="text-[12px] text-text-primary truncate">{session.title}</div>
                        <div className="text-[10px] text-text-tertiary mt-0.5">
                          {session.messages.filter(m => m.role === 'user').length} 条消息 · {formatTime(session.createdAt)}
                        </div>
                      </div>
                      <button
                        onClick={(e) => handleDeleteSession(e, session.id)}
                        className="p-1 rounded text-text-tertiary opacity-0 group-hover:opacity-100 hover:text-danger hover:bg-danger-muted transition-all shrink-0"
                        title="删除会话"
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  ))
                )}
              </div>
            )}
          </div>
          <button
            onClick={handleNewTask}
            className="p-1.5 rounded-md text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-all"
            title={`${t('chat.newTask')} (Ctrl+L)`}
          >
            <Plus size={16} />
          </button>
        </div>
      </div>

      {usage && <UsageBar usage={usage} />}

      {/* Messages */}
      <div className="flex-1 overflow-y-auto px-4 py-4 space-y-5">
        {messages.length === 0 && (
          <div className="h-full flex flex-col items-center justify-center text-center animate-fade-in">
            <div className="w-14 h-14 rounded-2xl bg-accent-muted flex items-center justify-center mb-4">
              <img src="/logo-b.png" alt="EspSmith" className="w-8 h-8" />
            </div>
            <h4 className="text-[15px] font-semibold mb-1">{t('chat.welcome')}</h4>
            <p className="text-[13px] text-text-tertiary max-w-[240px] leading-relaxed">
              {t('chat.welcomeDesc')}
            </p>
          </div>
        )}
        {groupedMessages.map((group) =>
          group.type === 'tool_group' ? (
            <ToolCallsGroup key={group.messages[0].id} messages={group.messages} />
          ) : (
            <MessageItem key={group.message.id} message={group.message} />
          )
        )}
        {activeOperation && (
          <OperationTimeline operation={activeOperation} />
        )}
        {/* Typing indicator */}
        {showTyping && (
          <div className="flex justify-start">
            <div className="bg-surface-overlay rounded-2xl rounded-bl-md px-4 py-3 border border-border-subtle">
              <div className="flex items-center gap-1">
                {[0, 1, 2].map((i) => (
                  <div
                    key={i}
                    className="w-1.5 h-1.5 rounded-full bg-text-tertiary"
                    style={{
                      animation: `typing-dot 1.4s ease-in-out ${i * 0.15}s infinite`,
                    }}
                  />
                ))}
              </div>
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {/* Input Area */}
      <div className="p-3 border-t border-border-default shrink-0">
        <div className="bg-surface-overlay rounded-xl border border-border-default focus-within:border-accent/50 focus-within:shadow-glow transition-all duration-200">
          <textarea
            ref={textareaRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t('chat.typeMessage')}
            rows={1}
            disabled={isBusy}
            className="w-full px-3 pt-2.5 pb-1 bg-transparent text-[13px] text-text-primary placeholder:text-text-disabled resize-none focus:outline-none disabled:opacity-50"
            style={{ minHeight: '36px', maxHeight: '200px' }}
          />
          <div className="flex items-center justify-between px-2 py-1.5">
            <div className="relative" ref={permDropdownRef}>
              <button
                onClick={() => setPermOpen(!permOpen)}
                className={`flex items-center gap-1 px-2 py-1 text-[11px] rounded-md transition-all ${
                  permissionMode === 'ask'
                    ? 'text-amber-700 bg-amber-100 hover:bg-amber-200'
                    : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-hover'
                }`}
                title={permissionMode === 'ask' ? '询问模式：敏感操作需确认' : '完全模式：不限制任何操作'}
              >
                {permissionMode === 'ask' ? <ShieldAlert size={12} /> : <Shield size={12} />}
                <span>{permissionMode === 'ask' ? '询问模式' : '完全模式'}</span>
                <ChevronDown size={10} className={`transition-transform ${permOpen ? 'rotate-180' : ''}`} />
              </button>
              {permOpen && (
                <div className="absolute left-0 bottom-full mb-1 w-[180px] bg-surface-elevated border border-border-default rounded-lg shadow-lg z-50 py-1 animate-scale-in origin-bottom-left">
                  <button
                    onClick={() => { setPermissionMode('full'); setPermOpen(false); }}
                    className={`w-full flex items-center gap-2 px-3 py-2 text-[12px] transition-colors ${
                      permissionMode === 'full'
                        ? 'bg-accent-muted text-accent'
                        : 'text-text-secondary hover:text-text-primary hover:bg-surface-hover'
                    }`}
                  >
                    <Shield size={13} />
                    <div className="text-left">
                      <div className="text-[12px]">完全模式</div>
                      <div className="text-[10px] text-text-tertiary">不限制任何操作</div>
                    </div>
                    {permissionMode === 'full' && <Check size={12} className="ml-auto text-accent" />}
                  </button>
                  <button
                    onClick={() => { setPermissionMode('ask'); setPermOpen(false); }}
                    className={`w-full flex items-center gap-2 px-3 py-2 text-[12px] transition-colors ${
                      permissionMode === 'ask'
                        ? 'bg-amber-100 text-amber-700'
                        : 'text-text-secondary hover:text-text-primary hover:bg-surface-hover'
                    }`}
                  >
                    <ShieldAlert size={13} />
                    <div className="text-left">
                      <div className="text-[12px]">询问模式</div>
                      <div className="text-[10px] text-text-tertiary">敏感操作需确认</div>
                    </div>
                    {permissionMode === 'ask' && <Check size={12} className="ml-auto text-amber-600" />}
                  </button>
                </div>
              )}
            </div>
            <button
              onClick={isBusy ? stopAI : handleSend}
              disabled={!isBusy && !input.trim()}
              className={`p-1.5 rounded-lg text-white transition-all shrink-0 ${
                isBusy
                  ? 'bg-error hover:bg-red-600 animate-pulse'
                  : 'bg-accent hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed'
              }`}
              title={isBusy ? t('chat.stop') : t('chat.send')}
            >
              {isBusy ? <StopCircle size={14} /> : <Send size={14} />}
            </button>
          </div>
        </div>

        {/* 回退确认对话框 */}
        {pendingRollback && (
          <div className="mt-3 p-4 bg-surface-elevated border border-warning rounded-xl animate-slide-up">
            <div className="flex items-center gap-2 mb-3">
              <Undo2 size={14} className="text-warning" />
              <span className="text-[13px] font-medium text-text-primary">
                确定退回到本次对话发起前吗？
              </span>
            </div>

            {pendingRollback.restoreFiles.length > 0 && (
              <div className="mb-2">
                <div className="text-[11px] text-text-secondary mb-1">以下文件将被恢复：</div>
                <div className="max-h-[100px] overflow-y-auto space-y-0.5">
                  {pendingRollback.restoreFiles.map((f) => (
                    <div key={f.path} className="flex items-center gap-1.5 pl-2 text-[11px]">
                      <span className="text-warning">↩</span>
                      <span className="text-text-tertiary truncate" title={f.path}>
                        {f.path.replace(/^.*[\\/]/, '')}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {pendingRollback.deleteFiles.length > 0 && (
              <div className="mb-2">
                <div className="text-[11px] text-text-secondary mb-1">以下文件将被删除：</div>
                <div className="max-h-[100px] overflow-y-auto space-y-0.5">
                  {pendingRollback.deleteFiles.map((f) => (
                    <div key={f.path} className="flex items-center gap-1.5 pl-2 text-[11px]">
                      <span className="text-error">✕</span>
                      <span className="text-text-tertiary truncate" title={f.path}>
                        {f.path.replace(/^.*[\\/]/, '')}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            <div className="flex gap-2 justify-end mt-3">
              <button
                onClick={cancelRollback}
                className="px-3 py-1 text-[12px] text-text-secondary hover:text-text-primary hover:bg-surface-hover rounded-md transition-all"
              >
                取消
              </button>
              <button
                onClick={confirmRollback}
                className="px-3 py-1 text-[12px] bg-warning text-white hover:bg-red-600 rounded-md transition-all font-medium"
              >
                确认回退
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function OperationTimeline({ operation }: { operation: OperationProgress }) {
  const [elapsed, setElapsed] = useState(0);
  const { t } = useTranslation();
  const isDone = operation.steps.every(s => s.status === 'done' || s.status === 'error');
  const hasError = operation.steps.some(s => s.status === 'error');

  useEffect(() => {
    if (isDone) return;
    const interval = setInterval(() => {
      setElapsed(Math.floor((Date.now() - (operation.startedAt || Date.now())) / 1000));
    }, 1000);
    return () => clearInterval(interval);
  }, [operation.startedAt, isDone]);

  const formatElapsed = (s: number) => {
    if (s < 60) return `${s}s`;
    return `${Math.floor(s / 60)}m ${s % 60}s`;
  };

  return (
    <div className="flex justify-center animate-slide-up">
      <div className="bg-surface-elevated border border-accent/30 rounded-xl px-4 py-3 max-w-[92%] w-full shadow-glow">
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2">
            {isDone && hasError ? (
              <XCircle size={14} className="text-error" />
            ) : isDone ? (
              <CheckCircle2 size={14} className="text-success" />
            ) : (
              <Cpu size={14} className="text-accent animate-pulse" />
            )}
            <span className="text-[12px] font-medium text-text-primary">{translateBackendString(operation.label)}</span>
          </div>
          <div className="flex items-center gap-1.5 text-[10px] text-text-tertiary">
            {!isDone && <Loader size={10} className="animate-spin text-accent" />}
            <span>{isDone ? (hasError ? t('aiOp.failed') : t('aiOp.completed')) : formatElapsed(elapsed)}</span>
          </div>
        </div>
        <div className="flex flex-col gap-1.5">
          {operation.steps.map((step, i) => {
            const isStepDone = step.status === 'done';
            const isStepError = step.status === 'error';
            const isRunning = !isDone && !isStepDone && !isStepError && (i === 0 || operation.steps[i - 1].status === 'done');
            return (
              <div key={i} className="flex items-center gap-2">
                {isStepDone ? (
                  <CheckCircle2 size={13} className="text-success shrink-0" />
                ) : isStepError ? (
                  <XCircle size={13} className="text-error shrink-0" />
                ) : isRunning ? (
                  <div className="w-[13px] h-[13px] rounded-full border-2 border-accent border-t-transparent animate-spin shrink-0" />
                ) : (
                  <Circle size={13} className="text-text-disabled shrink-0" />
                )}
                <span className={`text-[11px] ${
                  isStepDone ? 'text-success' : isStepError ? 'text-error' : isRunning ? 'text-accent font-medium' : 'text-text-disabled'
                }`}>
                  {translateBackendString(step.label)}
                </span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function ToolCallsGroup({ messages }: { messages: ChatMessage[] }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="flex flex-col items-center">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1.5 px-3 py-1.5 bg-surface-elevated border border-border-subtle rounded-lg text-[11px] text-text-tertiary hover:text-text-secondary hover:border-border-default transition-all"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Terminal size={12} />
        <span>AI 执行过程 · {messages.length} steps</span>
      </button>
      {expanded && (
        <div className="mt-2 px-3 py-2 bg-surface-root border border-border-subtle rounded-lg max-w-[92%] max-h-[320px] overflow-y-auto w-full animate-slide-up">
          {messages.map((msg, i) => (
            <ToolCallDetail key={msg.id} message={msg} isLast={i === messages.length - 1} />
          ))}
        </div>
      )}
    </div>
  );
}

function ToolCallDetail({ message, isLast }: { message: ChatMessage; isLast: boolean }) {
  const toolInput = message.toolData?.input as Record<string, unknown> | undefined;
  const toolResult = message.toolData?.result;
  const hasDetails = toolInput && Object.keys(toolInput).length > 0;
  const label = message.content.replace(/^🔧 /, '');
  const isFileTool = ['read_file', 'write_file'].includes(message.toolData?.name || '');
  const filePath = isFileTool ? resolveFilePath(toolInput) : null;
  const [toolCopied, setToolCopied] = useState(false);

  const isExecShell = message.toolData?.name === 'exec_shell';
  const execCmd = isExecShell ? String(toolInput?.command || '') : '';
  const isEspCli = isExecShell && execCmd.toLowerCase().includes('espsmith');

  const handleOpenFile = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (filePath) {
      useFileStore.getState().openFile(filePath);
    }
  };

  const toolFullText = [
    label,
    hasDetails ? `输入: ${JSON.stringify(toolInput)}` : '',
    toolResult ? `输出: ${toolResult}` : '',
  ].filter(Boolean).join('\n');

  return (
    <div className={`${isLast ? '' : 'border-b border-border-subtle pb-2 mb-2'}`}>
      <div className="flex items-center gap-1.5">
        <span className="text-[11px] font-medium text-text-secondary">{label}</span>
        {filePath && (
          <button
            onClick={handleOpenFile}
            className="px-1 py-0.5 text-[10px] text-accent hover:text-accent-hover hover:bg-accent-muted rounded transition-all flex items-center gap-0.5"
            title={`打开 ${filePath}`}
          >
            <ExternalLink size={10} />
          </button>
        )}
        <button
          onClick={() => doCopy(toolFullText, setToolCopied)}
          className="opacity-0 hover:opacity-100 px-1 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary rounded transition-all"
          title="复制工具调用详情"
        >
          {toolCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
        </button>
      </div>
      {hasDetails && !isEspCli && (
        <div className="mt-0.5 text-[10px] text-text-tertiary font-mono whitespace-pre-wrap opacity-70">
          {JSON.stringify(toolInput, null, 2)}
        </div>
      )}
      {isEspCli && toolResult && (
        <TerminalPanel command={execCmd} output={toolResult} />
      )}
      {!isEspCli && toolResult && (
        <div className="mt-0.5 text-[10px] text-text-tertiary font-mono whitespace-pre-wrap opacity-60 max-h-[80px] overflow-y-auto">
          {toolResult}
        </div>
      )}
    </div>
  );
}

function TerminalPanel({ command, output }: { command: string; output: string }) {
  const [expanded, setExpanded] = useState(false);
  const lines = output.split('\n');
  const preview = lines.slice(0, 3).join('\n');
  const hasMore = lines.length > 3;

  return (
    <div className="mt-1.5 rounded-lg overflow-hidden border border-[#333] bg-[#1e1e1e]">
      <div className="flex items-center justify-between px-3 py-1.5 bg-[#2d2d2d] border-b border-[#333]">
        <div className="flex items-center gap-1.5">
          <Terminal size={11} className="text-[#6a9955]" />
          <span className="text-[10px] font-mono text-[#cccccc] truncate max-w-[280px]">{command}</span>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => doCopy(output, () => {})}
            className="px-1.5 py-0.5 text-[10px] text-[#858585] hover:text-[#cccccc] rounded transition-colors"
            title="复制输出"
          >
            <Copy size={10} />
          </button>
          {hasMore && (
            <button
              onClick={() => setExpanded(!expanded)}
              className="px-1.5 py-0.5 text-[10px] text-[#858585] hover:text-[#cccccc] rounded transition-colors"
            >
              {expanded ? '收起' : `展开全部 ${lines.length} 行`}
            </button>
          )}
        </div>
      </div>
      <div className={`px-3 py-2 overflow-y-auto font-mono text-[11px] leading-relaxed text-[#d4d4d4] whitespace-pre-wrap ${expanded ? 'max-h-[400px]' : 'max-h-[100px]'}`}>
        {expanded ? output : preview}
        {!expanded && hasMore && (
          <div className="text-[#6a9955] mt-1">... {lines.length - 3} more lines</div>
        )}
      </div>
    </div>
  );
}

function doCopy(text: string, setter: (v: boolean) => void) {
  navigator.clipboard.writeText(text);
  setter(true);
  setTimeout(() => setter(false), 2000);
}

function MessageItem({ message }: { message: ChatMessage }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [localCopied, setLocalCopied] = useState(false);
  const [userCopied, setUserCopied] = useState(false);
  const [toolMsgCopied, setToolMsgCopied] = useState(false);
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  const isTool = !!message.toolData;

  // 工具调用消息 - 可折叠
  if (isTool) {
    const toolInput = message.toolData?.input as Record<string, unknown> | undefined;
    const toolResult = message.toolData?.result;
    const hasDetails = toolInput && Object.keys(toolInput).length > 0;
    const label = message.content.replace(/^🔧 /, '');
    const isFileTool = ['read_file', 'write_file'].includes(message.toolData?.name || '');
    const filePath = isFileTool ? resolveFilePath(toolInput) : null;

    const isExecShell = message.toolData?.name === 'exec_shell';
    const execCmd = isExecShell ? String(toolInput?.command || '') : '';
    const isEspCli = isExecShell && execCmd.toLowerCase().includes('espsmith');

    const handleOpenFile = (e: React.MouseEvent) => {
      e.stopPropagation();
      if (filePath) {
        useFileStore.getState().openFile(filePath);
      }
    };

    const toolMsgFullText = [
      label,
      hasDetails ? `输入: ${JSON.stringify(toolInput)}` : '',
      toolResult ? `输出: ${toolResult}` : '',
    ].filter(Boolean).join('\n');

    if (isEspCli && toolResult) {
      return (
        <div className="flex flex-col items-center w-full max-w-[92%]">
          <div className="flex items-center gap-1 mb-1">
            <span className="text-[11px] text-text-tertiary">{label}</span>
            <button
              onClick={() => doCopy(toolMsgFullText, setToolMsgCopied)}
              className="px-1 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary rounded transition-all"
              title="复制"
            >
              {toolMsgCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
            </button>
          </div>
          <TerminalPanel command={execCmd} output={toolResult} />
        </div>
      );
    }

    return (
      <div className="flex flex-col items-center">
        <div className="flex items-center gap-1">
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-1.5 px-2.5 py-1 bg-surface-elevated border border-border-subtle rounded-lg text-[11px] text-text-tertiary hover:text-text-secondary hover:border-border-default transition-all"
          >
            {(hasDetails || toolResult) && (expanded
              ? <ChevronDown size={12} />
              : <ChevronRight size={12} />
            )}
            <span>{label}</span>
          </button>
          {filePath && (
            <button
              onClick={handleOpenFile}
              className="px-1.5 py-0.5 text-[10px] text-accent hover:text-accent-hover hover:bg-accent-muted rounded transition-all flex items-center gap-0.5"
              title={`打开 ${filePath}`}
            >
              <ExternalLink size={10} />
            </button>
          )}
          <button
            onClick={() => doCopy(toolMsgFullText, setToolMsgCopied)}
            className="px-1 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary rounded transition-all"
            title="复制工具调用详情"
          >
            {toolMsgCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
          </button>
        </div>
        {expanded && (
          <div className="mt-1 px-3 py-2 bg-surface-root border border-border-subtle rounded-lg text-[11px] text-text-tertiary font-mono whitespace-pre-wrap max-h-[150px] overflow-y-auto max-w-[90%]">
            {hasDetails && <div className="text-text-secondary mb-1">输入:</div>}
            {hasDetails && <div>{JSON.stringify(toolInput, null, 2)}</div>}
            {toolResult && (
              <>
                <div className="text-text-secondary mt-2 mb-1">输出:</div>
                <div>{toolResult}</div>
              </>
            )}
          </div>
        )}
      </div>
    );
  }

  // 普通系统消息
  if (isSystem) {
    return (
      <div className="flex justify-center group">
        <div className="flex items-center gap-1.5 px-3 py-1 bg-surface-elevated border border-border-subtle rounded-full text-[11px] text-text-tertiary">
          <span>{message.content}</span>
          <button
            onClick={() => doCopy(message.content, setUserCopied)}
            className="opacity-0 group-hover:opacity-100 transition-opacity text-text-tertiary hover:text-text-primary"
            title="复制消息"
          >
            {userCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={`flex gap-3 ${isUser ? 'flex-row-reverse' : 'flex-row'} animate-slide-up`}>
      {/* Avatar */}
      <div className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 mt-0.5 ${
        isUser ? 'bg-accent' : 'bg-surface-overlay border border-border-subtle'
      }`}>
        {isUser
          ? <User size={13} className="text-white" />
          : <Bot size={13} className="text-accent" />
        }
      </div>

      {/* Bubble */}
      <div className={`max-w-[80%] ${isUser ? 'items-end' : 'items-start'}`}>
        <div className="flex items-center gap-2 mb-1">
          <span className="text-[11px] font-medium text-text-secondary">
            {isUser ? t('chat.you') : t('chat.assistant')}
          </span>
          {isUser && message.id.startsWith('user-') && (
            <button
              onClick={() => useChatStore.getState().prepareRollback(message.id)}
              className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] text-text-tertiary hover:text-warning hover:bg-warning-muted rounded transition-all"
              title="回退到本次对话前"
            >
              <Undo2 size={10} />
              回退
            </button>
          )}
          {isUser && message.id.startsWith('user-') && (
            <button
              onClick={() => {
                const textarea = document.querySelector('textarea') as HTMLTextAreaElement;
                if (textarea) {
                  textarea.value = message.content;
                  textarea.focus();
                  textarea.setSelectionRange(textarea.value.length, textarea.value.length);
                }
              }}
              className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] text-text-tertiary hover:text-accent hover:bg-accent-muted rounded transition-all"
              title="重新编辑"
            >
              <Pencil size={10} />
            </button>
          )}
          {isUser && (
            <button
              onClick={() => doCopy(message.content, setUserCopied)}
              className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary hover:bg-surface-hover rounded transition-all"
              title="复制消息"
            >
              {userCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
            </button>
          )}
        </div>
        <div className={`px-4 py-2.5 text-[13px] leading-relaxed ${
          isUser
            ? 'bg-accent text-white rounded-2xl rounded-tr-sm'
            : 'bg-surface-overlay border border-border-subtle rounded-2xl rounded-tl-sm'
        }`}>
          <MessageContent content={message.content} />
        </div>
        {!isUser && message.content && (
          <div className="flex items-center gap-1 mt-1">
            <button
              onClick={() => doCopy(message.content, setLocalCopied)}
              className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary hover:bg-surface-hover rounded transition-all"
              title="复制消息"
            >
              {localCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
              复制
            </button>
          </div>
        )}
        {!isUser && message.usage && (
          <div className="flex items-center gap-1.5 mt-0.5 text-[10px] text-text-disabled">
            <span>↑{formatTokens(message.usage.inputTokens)} ↓{formatTokens(message.usage.outputTokens)}</span>
          </div>
        )}
      </div>
    </div>
  );
}

function CodeBlock({ language, code, content }: { language: string; code: string; content: string }) {
  const { t } = useTranslation();
  const [copyOk, setCopyOk] = useState(false);
  const [applying, setApplying] = useState(false);

  const extractFilePath = (): string | null => {
    const idx = content.indexOf(code);
    if (idx === -1) return null;
    const before = content.slice(Math.max(0, idx - 300), idx);
    const pattern = /(?:文件|path|file)[:：\s]*[`'"]?([^\s`'"\n]{2,200})/i;
    const match = before.match(pattern);
    if (match) {
      let path = match[1].replace(/[`'"]/g, '').trim();
      if (path.startsWith('.') || !path.includes(':')) {
        return path;
      }
    }
    const pathPattern = /([\w./-]+\.[chCH]{1,2}(?:pp)?)/;
    const pathMatch = before.match(pathPattern);
    if (pathMatch) {
      return pathMatch[1];
    }
    return null;
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(code);
    setCopyOk(true);
    setTimeout(() => setCopyOk(false), 2000);
  };

  const handleApply = async () => {
    const suggested = extractFilePath();
    const filePath = prompt('请输入文件路径（相对于项目根目录）:', suggested || '');
    if (!filePath) return;

    setApplying(true);
    try {
      if (typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('write_file', { path: filePath, content: code, safeMode: false });
        useFileStore.getState().openFile(filePath);
      } else {
        alert('请在 Tauri 桌面应用中体验完整功能');
      }
    } catch (err) {
      alert(`写入失败: ${err}`);
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="my-2 rounded-lg bg-surface-root border border-border-subtle overflow-hidden">
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-border-subtle">
        <div className="flex items-center gap-1.5">
          <Code size={11} className="text-text-tertiary" />
          <span className="text-[10px] text-text-tertiary uppercase">{language}</span>
        </div>
        <div className="flex items-center gap-1">
          <button
            className="flex items-center gap-1 text-[10px] text-text-tertiary hover:text-text-primary transition-colors"
            onClick={handleCopy}
          >
            {copyOk ? <Check size={12} className="text-success" /> : <Copy size={12} />}
            <span>{copyOk ? t('common.copied') : t('common.copy')}</span>
          </button>
          <button
            className="flex items-center gap-1 px-2 py-0.5 text-[10px] bg-accent text-white rounded hover:bg-accent-hover transition-colors disabled:opacity-50"
            onClick={handleApply}
            disabled={applying}
          >
            {applying ? '写入中...' : 'Apply'}
          </button>
        </div>
      </div>
      <SyntaxHighlighter
        style={oneDark}
        language={language}
        PreTag="div"
        customStyle={{ margin: 0, background: 'transparent', padding: '0.75rem', fontSize: '12px', lineHeight: '1.6' }}
      >
        {code}
      </SyntaxHighlighter>
    </div>
  );
}

function MessageContent({ content }: { content: string }) {
  return (
    <div className="[&_p]:mb-2 [&_p:last-of-type]:mb-0 [&_ul]:list-disc [&_ul]:pl-5 [&_ul]:mb-2 [&_ul]:space-y-1 [&_ol]:list-decimal [&_ol]:pl-5 [&_ol]:mb-2 [&_ol]:space-y-1 [&_h1]:text-lg [&_h1]:font-bold [&_h1]:mb-2 [&_h2]:text-base [&_h2]:font-bold [&_h2]:mb-2 [&_h3]:text-sm [&_h3]:font-bold [&_h3]:mb-1 [&_table]:border-collapse [&_table]:mb-2 [&_table]:text-[12px] [&_th]:border [&_th]:border-border-subtle [&_th]:px-2 [&_th]:py-1 [&_th]:bg-surface-overlay [&_th]:font-medium [&_td]:border [&_td]:border-border-subtle [&_td]:px-2 [&_td]:py-1 [&_blockquote]:border-l-2 [&_blockquote]:border-accent [&_blockquote]:pl-3 [&_blockquote]:italic [&_blockquote]:text-text-tertiary [&_blockquote]:mb-2 [&_a]:text-accent [&_a]:hover:underline [&_hr]:border-border-subtle [&_hr]:my-2">
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkBreaks]}
      components={{
        code({ node, className, children, ...props }) {
          const match = /language-(\w+)/.exec(className || '');
          const codeString = String(children).replace(/\n$/, '');
          if (match) {
            return <CodeBlock language={match[1]} code={codeString} content={content} />;
          }
          return <code className="px-1 py-0.5 rounded bg-surface-overlay text-text-secondary font-mono text-[12px]" {...props}>{children}</code>;
        },
      }}
    >
      {content}
    </ReactMarkdown>
    </div>
  );
}

export default ChatPanel;