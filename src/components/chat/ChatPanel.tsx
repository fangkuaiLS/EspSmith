/**
 * ChatPanel - AI 聊天面板组件 (Codex-inspired)
 *
 * 功能：
 * - 消息列表显示（Markdown 渲染）
 * - 消息输入框（支持 Enter 发送）
 * - AI 状态指示（脉动圆点）
 * - 快捷命令
 */

import { useState, useEffect, useRef, useMemo, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { translateBackendString } from '../../i18n';
import { Send, StopCircle, Plus, User, Code, Terminal, ChevronDown, ChevronRight, ExternalLink, Undo2, Coins, Copy, Check, Pencil, Clock, Trash2, Shield, ShieldAlert, Cpu, Loader, CheckCircle2, Circle, XCircle, Brain } from 'lucide-react';
import { useChatStore, useSettingsStore } from '../../stores';
import { showToast } from '../ui/Toast';
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
import { open } from '@tauri-apps/plugin-shell';
import { getCurrentWebview } from '@tauri-apps/api/webview';
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



interface ToolchainOption {
  id: string;
  label: string;
  aiModel: 'deepseek' | 'ollama' | 'mimo';
}

const TOOLCHAIN_OPTIONS: ToolchainOption[] = [
  { id: 'codewhale', label: 'CodeWhale', aiModel: 'deepseek' },
  { id: 'mimo', label: 'MiMo-Code', aiModel: 'mimo' },
  // Ollama 暂时禁用，待独立实现
  // { id: 'ollama', label: 'Ollama', aiModel: 'ollama' },
];

interface ModelOption {
  id: string;
  label: string;
  labelKey?: string;
  model: string;
}

// 每个工具链对应的模型列表
const MODELS_BY_TOOLCHAIN: Record<string, ModelOption[]> = {
  codewhale: [
    { id: 'deepseek-v4-pro', label: 'DeepSeek V4 Pro', model: 'deepseek-v4-pro' },
    { id: 'deepseek-v4-flash', label: 'DeepSeek V4 Flash', model: 'deepseek-v4-flash' },
  ],
  mimo: [
    { id: 'mimo/mimo-auto', label: '', labelKey: 'chat.model.mimoAuto', model: 'mimo/mimo-auto' },
  ],
  ollama: [
    { id: 'ollama', label: 'Ollama (Local)', model: 'ollama' },
  ],
};

function getCurrentToolchainId(): string {
  const s = useSettingsStore.getState().settings;
  if (s.aiModel === 'mimo') return 'mimo';
  if (s.aiModel === 'ollama') return 'ollama';
  return 'codewhale';
}

function getCurrentModelId(): string {
  const s = useSettingsStore.getState().settings;
  if (s.aiModel === 'ollama') return 'ollama';
  if (s.aiModel === 'mimo') return s.mimoModel || 'mimo/mimo-auto';
  return s.deepseekModel || 'deepseek-v4-pro';
}

function formatTime(ts: number, t: (key: string, opts?: Record<string, unknown>) => string): string {
  const date = new Date(ts);
  const now = new Date();
  const diff = now.getTime() - date.getTime();
  if (diff < 60000) return t('chat.time.justNow');
  if (diff < 3600000) return t('chat.time.minutesAgo', { n: Math.floor(diff / 60000) });
  if (diff < 86400000) return t('chat.time.hoursAgo', { n: Math.floor(diff / 3600000) });
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
            <span className="text-green-400" title={t('chat.tokens.cacheHit')}>
              ⊞{formatTokens(usage.lastMessage.cachedTokens)}
            </span>
          )}
        </>
      )}
    </div>
  );
}

function ChatPanel() {
  const { t } = useTranslation();
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const inputAreaRef = useRef<HTMLDivElement>(null);
  const [isDragOver, setIsDragOver] = useState(false);
  const isDragOverRef = useRef(false);
  const [modelOpen, setModelOpen] = useState(false);
  // Apply 对话框状态提升到顶层，避免 CodeBlock 重建时丢失
  const [applyDialog, setApplyDialog] = useState<{ code: string; suggested: string } | null>(null);
  const [applyFilePath, setApplyFilePath] = useState('');
  const [applyWriting, setApplyWriting] = useState(false);
  const modelDropdownRef = useRef<HTMLDivElement>(null);
  const [toolchainOpen, setToolchainOpen] = useState(false);
  const toolchainDropdownRef = useRef<HTMLDivElement>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  const historyDropdownRef = useRef<HTMLDivElement>(null);
  const [permOpen, setPermOpen] = useState(false);
  const permDropdownRef = useRef<HTMLDivElement>(null);

  const inputHistoryRef = useRef<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const draftInputRef = useRef('');

  const { messages, status, pendingRollback, usage, sessions, permissionMode, pendingPermission, activeOperation, messageQueue, sendMessage, startAI, stopAI, clearMessages, confirmRollback, cancelRollback, loadSessions, loadSession, deleteSession, setPermissionMode, enqueueMessage, clearQueue } = useChatStore();
  const { settings, setSettings } = useSettingsStore();
  const projectPath = useProjectStore((s) => s.currentProject?.path);

  // Apply 确认写入
  const confirmApply = async () => {
    if (!applyFilePath.trim() || !applyDialog) return;
    const code = applyDialog.code;
    const filePath = applyFilePath.trim();
    setApplyDialog(null);
    setApplyWriting(true);
    try {
      if (typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('write_file', { path: filePath, content: code, safeMode: false });
        useFileStore.getState().openFile(filePath);
      } else {
        showToast('warning', t('chat.error.desktopOnly'));
      }
    } catch (err) {
      showToast('error', t('chat.error.writeFailed', { error: String(err) }));
    } finally {
      setApplyWriting(false);
    }
  };

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

  // 文件/文件夹拖放支持：将拖入的路径插入到输入框
  useEffect(() => {
    if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window || '__TAURI__' in window)) return;
    let unlisten: (() => void) | undefined;

    async function setupDragDrop() {
      try {
        const webview = getCurrentWebview();
        unlisten = await webview.onDragDropEvent((event) => {
          const payload = event.payload;
          const dpr = window.devicePixelRatio || 1;

          if (payload.type === 'enter' || payload.type === 'over') {
            // 物理像素 → CSS 像素，与 getBoundingClientRect 对齐
            const cssX = payload.position.x / dpr;
            const cssY = payload.position.y / dpr;
            const area = inputAreaRef.current;
            if (area) {
              const rect = area.getBoundingClientRect();
              const inside = cssX >= rect.left && cssX <= rect.right && cssY >= rect.top && cssY <= rect.bottom;
              isDragOverRef.current = inside;
              setIsDragOver(inside);
            }
          } else if (payload.type === 'drop') {
            if (isDragOverRef.current) {
              const paths = payload.paths;
              if (paths && paths.length > 0) {
                // 用引号包裹路径（兼容空格），多个路径用空格连接
                const pathText = paths.map(p => `"${p}"`).join(' ');
                const textarea = textareaRef.current;
                if (textarea) {
                  const start = textarea.selectionStart;
                  const end = textarea.selectionEnd;
                  setInput(prev => prev.slice(0, start) + pathText + prev.slice(end));
                  setTimeout(() => {
                    textarea.selectionStart = textarea.selectionEnd = start + pathText.length;
                    textarea.focus();
                  }, 0);
                } else {
                  setInput(prev => prev + pathText);
                }
              }
            }
            isDragOverRef.current = false;
            setIsDragOver(false);
          } else if (payload.type === 'leave') {
            isDragOverRef.current = false;
            setIsDragOver(false);
          }
        });
      } catch { /* ignore */ }
    }
    setupDragDrop();
    return () => { unlisten?.(); };
  }, []);

  const handleToolchainChange = useCallback(async (option: ToolchainOption) => {
    setToolchainOpen(false);
    const currentToolchain = getCurrentToolchainId();
    if (option.id === currentToolchain) return;
    const newSettings = { ...settings };
    newSettings.aiModel = option.aiModel;
    setSettings(newSettings);
    stopAI();
    clearMessages(true);
    loadSessions();
    setTimeout(() => { startAI(); }, 150);
  }, [settings, setSettings, stopAI, clearMessages, startAI, loadSessions]);

  const handleModelSelect = useCallback(async (option: ModelOption) => {
    setModelOpen(false);
    const currentModel = getCurrentModelId();
    if (option.id === currentModel) return;
    const newSettings = { ...settings };
    const toolchain = getCurrentToolchainId();
    if (toolchain === 'mimo') {
      newSettings.mimoModel = option.model;
    } else {
      newSettings.deepseekModel = option.model as 'deepseek-v4-pro' | 'deepseek-v4-flash';
    }
    setSettings(newSettings);
    stopAI();
    clearMessages(true);
    loadSessions();
    setTimeout(() => { startAI(); }, 150);
  }, [settings, setSettings, stopAI, clearMessages, startAI, loadSessions]);

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
    const content = input.trim();
    inputHistoryRef.current.push(content);
    setHistoryIndex(-1);
    draftInputRef.current = '';
    setInput('');
    if (isBusy) {
      // 当前有任务在执行，将消息加入等待队列
      enqueueMessage(content);
    } else {
      await sendMessage(content);
    }
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

      {/* Apply 文件路径对话框 - 在 ChatPanel 顶层渲染，避免 CodeBlock 重建时关闭 */}
      {applyDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" onClick={() => setApplyDialog(null)}>
          <div
            className="bg-surface-elevated border border-border-default rounded-xl shadow-2xl w-[400px] max-w-[90vw] p-4 animate-slide-up"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="text-[13px] font-medium text-text-primary mb-3">
              {t('chat.apply.filePathPrompt')}
            </div>
            <input
              autoFocus
              value={applyFilePath}
              onChange={(e) => setApplyFilePath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') confirmApply();
                if (e.key === 'Escape') setApplyDialog(null);
              }}
              className="w-full px-3 py-2 bg-surface-overlay border border-border-default rounded-lg text-[13px] text-text-primary focus:outline-none focus:border-accent"
              placeholder={t('chat.apply.filePathPlaceholder')}
            />
            <div className="flex justify-end gap-2 mt-4">
              <button
                onClick={() => setApplyDialog(null)}
                className="px-3 py-1.5 text-[12px] text-text-secondary hover:text-text-primary transition-colors rounded-lg hover:bg-surface-hover"
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={confirmApply}
                disabled={!applyFilePath.trim() || applyWriting}
                className="px-3 py-1.5 text-[12px] bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {applyWriting ? t('chat.apply.writing') : t('common.ok')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Header */}
      <div className="px-4 py-3 border-b border-border-default flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="h-7 w-28 flex items-center justify-center rounded-md bg-white/90 dark:bg-white/80 p-1">
            <img src={`/icons/${getCurrentToolchainId() === 'mimo' ? 'mimo-code' : getCurrentToolchainId()}.svg`} alt="" className="w-full h-full object-contain" />
          </div>
          <div className="flex items-center gap-1.5">
            <div className={`w-1.5 h-1.5 rounded-full ${statusConfig.dotClass}`} />
            <span className="text-[11px] text-text-tertiary">{t(statusConfig.labelKey)}</span>
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          {/* 工具链选择 */}
          <div className="relative" ref={toolchainDropdownRef}>
            <button
              onClick={() => setToolchainOpen(!toolchainOpen)}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-surface-overlay border border-border-subtle text-[11px] text-text-secondary hover:text-text-primary hover:border-border-default transition-all"
              title={t('chat.toolchain.switch')}
            >
              <span className="max-w-[100px] truncate">
                {TOOLCHAIN_OPTIONS.find(o => o.id === getCurrentToolchainId())?.label || 'CodeWhale'}
              </span>
              <ChevronDown size={11} className={`transition-transform ${toolchainOpen ? 'rotate-180' : ''}`} />
            </button>
            {toolchainOpen && (
              <div className="absolute right-0 top-full mt-1 w-[180px] bg-surface-elevated border border-border-default rounded-lg shadow-lg z-50 py-1 animate-scale-in origin-top-right">
                {TOOLCHAIN_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    onClick={() => handleToolchainChange(option)}
                    className={`w-full flex items-center gap-2 px-3 py-2 text-[12px] transition-colors ${
                      option.id === getCurrentToolchainId()
                        ? 'bg-accent-muted text-accent'
                        : 'text-text-secondary hover:text-text-primary hover:bg-surface-hover'
                    }`}
                  >
                    <div className={`w-2 h-2 rounded-full ${option.id === getCurrentToolchainId() ? 'bg-accent' : 'bg-text-disabled'}`} />
                    <span>{option.label}</span>
                    {option.id === getCurrentToolchainId() && <Check size={12} className="ml-auto text-accent" />}
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
              title={t('chat.history.title')}
            >
              <Clock size={16} />
            </button>
            {historyOpen && (
              <div className="absolute right-0 top-full mt-1 w-[280px] bg-surface-elevated border border-border-default rounded-lg shadow-lg z-50 py-1 animate-scale-in origin-top-right max-h-[400px] overflow-y-auto">
                {sessions.length === 0 ? (
                  <div className="px-4 py-6 text-center text-[12px] text-text-tertiary">
                    {t('chat.history.empty')}
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
                          {t('chat.history.messageCount', { n: session.messages.filter(m => m.role === 'user').length })} {formatTime(session.createdAt, t)}
                        </div>
                      </div>
                      <button
                        onClick={(e) => handleDeleteSession(e, session.id)}
                        className="p-1 rounded text-text-tertiary opacity-0 group-hover:opacity-100 hover:text-danger hover:bg-danger-muted transition-all shrink-0"
                        title={t('chat.history.delete')}
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
            <MessageItem key={group.message.id} message={group.message} onApply={(code, suggested) => {
              setApplyDialog({ code, suggested });
              setApplyFilePath(suggested);
            }} />
          )
        )}
        {activeOperation && (
          <OperationTimeline operation={activeOperation} />
        )}
        {/* Thinking Block — AI 响应期间始终显示，可折叠查看详情 */}
        {showTyping && <ThinkingBlock status={status} messages={messages} />}
        <div ref={messagesEndRef} />
      </div>

      {/* Input Area */}
      <div className="p-3 border-t border-border-default shrink-0">
        {/* 消息队列 - 输入框上方 */}
        {messageQueue.length > 0 && (
          <div className="mb-2 space-y-1.5">
            <div className="flex items-center justify-between px-1">
              <span className="text-[11px] text-text-tertiary flex items-center gap-1">
                <Clock size={11} className="text-warning" />
                {t('chat.queuedCount', { count: messageQueue.length })}
              </span>
              <button
                onClick={clearQueue}
                className="text-[10px] text-text-tertiary hover:text-danger transition-colors flex items-center gap-0.5"
                title={t('chat.clearQueue')}
              >
                <Trash2 size={10} />
                <span>{t('chat.clearQueue')}</span>
              </button>
            </div>
            {messageQueue.map((msg, idx) => (
              <div
                key={idx}
                className="flex items-center gap-2 px-2.5 py-1.5 bg-surface-hover/50 border border-border-subtle rounded-lg group"
              >
                <div className="w-4 h-4 rounded-full bg-warning/20 flex items-center justify-center shrink-0">
                  <span className="text-[9px] text-warning font-medium">{idx + 1}</span>
                </div>
                <span className="text-[11px] text-text-secondary truncate flex-1" title={msg}>
                  {msg}
                </span>
                {idx === 0 && (
                  <span className="text-[9px] text-warning bg-warning/10 px-1.5 py-0.5 rounded shrink-0">
                    {t('chat.queuedNext')}
                  </span>
                )}
              </div>
            ))}
          </div>
        )}

        <div
          ref={inputAreaRef}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => e.preventDefault()}
          className={`bg-surface-overlay rounded-xl border transition-all duration-200 relative ${
            isDragOver
              ? 'border-accent shadow-glow ring-2 ring-accent/30'
              : 'border-border-default focus-within:border-accent/50 focus-within:shadow-glow'
          }`}
        >
          {isDragOver && (
            <div className="absolute inset-0 z-10 flex items-center justify-center rounded-xl bg-accent-muted/40 pointer-events-none">
              <span className="text-[12px] font-medium text-accent px-3 py-1.5 rounded-full bg-surface-elevated border border-accent/40 shadow">
                {t('chat.input.dropHint')}
              </span>
            </div>
          )}
          <textarea
            ref={textareaRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={isBusy ? t('chat.typeMessageQueued') : t('chat.typeMessage')}
            rows={1}
            className="w-full px-3 pt-2.5 pb-1 bg-transparent text-[13px] text-text-primary placeholder:text-text-disabled resize-none focus:outline-none"
            style={{ minHeight: '36px', maxHeight: '200px' }}
          />
          <div className="flex items-center justify-between px-2 py-1.5">
            <div className="flex items-center gap-1">
              {/* 模型选择 */}
              <div className="relative" ref={modelDropdownRef}>
                <button
                  onClick={() => setModelOpen(!modelOpen)}
                  className="flex items-center gap-1 px-2 py-1 text-[11px] rounded-md text-text-tertiary hover:text-text-secondary hover:bg-surface-hover transition-all"
                  title={t('chat.model.switch')}
                >
                  <Cpu size={12} />
                  <span className="max-w-[80px] truncate">
                    {(() => {
                      const opt = MODELS_BY_TOOLCHAIN[getCurrentToolchainId()]?.find(o => o.id === getCurrentModelId());
                      return opt ? (opt.labelKey ? t(opt.labelKey) : opt.label) : getCurrentModelId();
                    })()}
                  </span>
                  <ChevronDown size={10} className={`transition-transform ${modelOpen ? 'rotate-180' : ''}`} />
                </button>
                {modelOpen && (
                  <div className="absolute left-0 bottom-full mb-1 w-[200px] bg-surface-elevated border border-border-default rounded-lg shadow-lg z-50 py-1 animate-scale-in origin-bottom-left">
                    {MODELS_BY_TOOLCHAIN[getCurrentToolchainId()]?.map((option) => (
                      <button
                        key={option.id}
                        onClick={() => handleModelSelect(option)}
                        className={`w-full flex items-center gap-2 px-3 py-2 text-[12px] transition-colors ${
                          option.id === getCurrentModelId()
                            ? 'bg-accent-muted text-accent'
                            : 'text-text-secondary hover:text-text-primary hover:bg-surface-hover'
                        }`}
                      >
                        <div className={`w-2 h-2 rounded-full ${option.id === getCurrentModelId() ? 'bg-accent' : 'bg-text-disabled'}`} />
                        <span>{option.labelKey ? t(option.labelKey) : option.label}</span>
                        {option.id === getCurrentModelId() && <Check size={12} className="ml-auto text-accent" />}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              {/* 权限模式 */}
              <div className="relative" ref={permDropdownRef}>
              <button
                onClick={() => setPermOpen(!permOpen)}
                className={`flex items-center gap-1 px-2 py-1 text-[11px] rounded-md transition-all ${
                  permissionMode === 'ask'
                    ? 'text-amber-700 bg-amber-100 hover:bg-amber-200'
                    : 'text-text-tertiary hover:text-text-secondary hover:bg-surface-hover'
                }`}
                title={permissionMode === 'ask' ? t('chat.permission.askModeDesc') : t('chat.permission.fullModeDesc')}
              >
                {permissionMode === 'ask' ? <ShieldAlert size={12} /> : <Shield size={12} />}
                <span>{permissionMode === 'ask' ? t('chat.permission.askMode') : t('chat.permission.fullMode')}</span>
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
                      <div className="text-[12px]">{t('chat.permission.fullMode')}</div>
                      <div className="text-[10px] text-text-tertiary">{t('chat.permission.fullModeDesc')}</div>
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
                      <div className="text-[12px]">{t('chat.permission.askMode')}</div>
                      <div className="text-[10px] text-text-tertiary">{t('chat.permission.askModeDesc')}</div>
                    </div>
                    {permissionMode === 'ask' && <Check size={12} className="ml-auto text-amber-600" />}
                  </button>
                </div>
              )}
            </div>
            </div>
            <div className="flex items-center gap-1.5 shrink-0">
              {isBusy && (
                <button
                  onClick={stopAI}
                  className="p-1.5 rounded-lg text-white bg-error hover:bg-red-600 animate-pulse transition-all"
                  title={t('chat.stop')}
                >
                  <StopCircle size={14} />
                </button>
              )}
              <button
                onClick={handleSend}
                disabled={!input.trim()}
                className={`p-1.5 rounded-lg text-white transition-all shrink-0 disabled:opacity-40 disabled:cursor-not-allowed ${
                  isBusy
                    ? 'bg-warning hover:bg-amber-600'
                    : 'bg-accent hover:bg-accent-hover'
                }`}
                title={isBusy ? t('chat.enqueue') : t('chat.send')}
              >
                <Send size={14} />
              </button>
            </div>
          </div>
        </div>

        {/* 回退确认对话框 */}
        {pendingRollback && (
          <div className="mt-3 p-4 bg-surface-elevated border border-warning rounded-xl animate-slide-up">
            <div className="flex items-center gap-2 mb-3">
              <Undo2 size={14} className="text-warning" />
              <span className="text-[13px] font-medium text-text-primary">
                {t('chat.rollback.confirmTitle')}
              </span>
            </div>

            {pendingRollback.restoreFiles.length > 0 && (
              <div className="mb-2">
                <div className="text-[11px] text-text-secondary mb-1">{t('chat.rollback.restoreFiles')}</div>
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
                <div className="text-[11px] text-text-secondary mb-1">{t('chat.rollback.deleteFiles')}</div>
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
                {t('common.cancel')}
              </button>
              <button
                onClick={confirmRollback}
                className="px-3 py-1 text-[12px] bg-warning text-white hover:bg-red-600 rounded-md transition-all font-medium"
              >
                {t('chat.rollback.confirm')}
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
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="flex flex-col items-center">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1.5 px-3 py-1.5 bg-surface-elevated border border-border-subtle rounded-lg text-[11px] text-text-tertiary hover:text-text-secondary hover:border-border-default transition-all"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <Terminal size={12} />
        <span>{t('chat.toolCalls.groupLabel', { n: messages.length })}</span>
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
  const { t } = useTranslation();
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
    hasDetails ? t('chat.toolCalls.input', { input: JSON.stringify(toolInput) }) : '',
    toolResult ? t('chat.toolCalls.output', { output: toolResult }) : '',
  ].filter(Boolean).join('\n');

  return (
    <div className={`${isLast ? '' : 'border-b border-border-subtle pb-2 mb-2'}`}>
      <div className="flex items-center gap-1.5">
        <span className="text-[11px] font-medium text-text-secondary">{label}</span>
        {filePath && (
          <button
            onClick={handleOpenFile}
            className="px-1 py-0.5 text-[10px] text-accent hover:text-accent-hover hover:bg-accent-muted rounded transition-all flex items-center gap-0.5"
            title={t('chat.toolCalls.openFile', { filePath })}
          >
            <ExternalLink size={10} />
          </button>
        )}
        <button
          onClick={() => doCopy(toolFullText, setToolCopied)}
          className="opacity-0 hover:opacity-100 px-1 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary rounded transition-all"
          title={t('chat.toolCalls.copyDetails')}
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
  const { t } = useTranslation();
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
            title={t('chat.toolCalls.copyOutput')}
          >
            <Copy size={10} />
          </button>
          {hasMore && (
            <button
              onClick={() => setExpanded(!expanded)}
              className="px-1.5 py-0.5 text-[10px] text-[#858585] hover:text-[#cccccc] rounded transition-colors"
            >
              {expanded ? t('chat.toolCalls.collapse') : t('chat.toolCalls.expandAll', { n: lines.length })}
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

/**
 * ThinkingBlock — AI 思考/推理过程的可折叠展示块
 * 在 AI 响应期间（showTyping）显示，替代原来的三个跳动圆点
 * 参考 MiMO-Code TUI 的 "- Thought: [title] · 22ms" 风格
 * 只展示 AI 的 reasoning/thinking 内容，不重复展示工具调用（工具调用已有 OperationTimeline）
 */
function ThinkingBlock({ status, messages }: { status: AIStatus; messages: ChatMessage[] }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const prevThinkingRef = useRef('');

  // 从最后一条 assistant 消息中获取 reasoning 内容（仅当前轮次）
  const thinkingContent = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role === 'assistant') {
        // 只看最后一条 assistant 消息，避免显示上一轮的 thinking
        return messages[i].thinkingContent || '';
      }
    }
    return '';
  }, [messages]);

  // thinking 内容到达时自动展开
  useEffect(() => {
    if (thinkingContent && thinkingContent !== prevThinkingRef.current) {
      prevThinkingRef.current = thinkingContent;
      if (!expanded) setExpanded(true);
    }
    // 新一轮对话 thinking 被清空时，折叠
    if (!thinkingContent && prevThinkingRef.current) {
      prevThinkingRef.current = '';
      setExpanded(false);
    }
  }, [thinkingContent]);

  // 解析 reasoning 文本：提取 **标题** 和正文（MiMO 格式）
  const { title, body } = useMemo(() => {
    const text = thinkingContent?.trim() || '';
    const match = text.match(/^\*\*([^*\n]+)\*\*(?:\r?\n\r?\n|$)/);
    if (match) {
      return { title: match[1].trim(), body: text.slice(match[0].length).trimEnd() };
    }
    return { title: null, body: text };
  }, [thinkingContent]);

  const hasThinking = !!thinkingContent;

  return (
    <div className="flex justify-start animate-slide-up">
      <div className="max-w-[85%]">
        <div className="border border-border-subtle rounded-xl overflow-hidden bg-surface-overlay/80 backdrop-blur-sm">
          {/* Header — 一行展示 */}
          <button
            onClick={() => hasThinking && setExpanded(!expanded)}
            className={`flex items-center gap-1.5 px-3 py-2 text-[12px] transition-colors ${hasThinking ? 'hover:bg-surface-hover cursor-pointer' : 'cursor-default'}`}
          >
            {/* 状态图标 */}
            <Brain size={13} className={`shrink-0 ${status === 'thinking' ? 'text-accent animate-pulse' : 'text-text-tertiary'}`} />

            {/* 标题文字 */}
            <span className="text-text-secondary font-medium">
              {hasThinking ? (
                <>{t('chat.thinking.thought')}{title && <>: </>}<span className="text-accent/90">{title || (body.slice(0, 40) + (body.length > 40 ? '...' : ''))}</span></>
              ) : (
                <span>{t('chat.thinking.process')}</span>
              )}
            </span>

            {/* 动画点 */}
            <div className="flex items-center gap-0.5 ml-auto mr-0.5">
              {[0, 1, 2].map((i) => (
                <div
                  key={i}
                  className="w-1 h-1 rounded-full bg-accent/40"
                  style={{ animation: `typing-dot 1.4s ease-in-out ${i * 0.15}s infinite` }}
                />
              ))}
            </div>

            {/* 展开/折叠箭头 */}
            {hasThinking && (
              expanded ? <ChevronDown size={12} className="text-text-disabled shrink-0" /> : <ChevronRight size={12} className="text-text-disabled shrink-0" />
            )}
          </button>

          {/* 展开内容：仅显示 Reasoning 正文 */}
          {expanded && hasThinking && (
            <div className="border-t border-border-subtle max-h-[300px] overflow-y-auto">
              <div className="px-3 py-2.5 text-[13px] leading-relaxed whitespace-pre-wrap bg-surface-elevated text-text-secondary">
                {body || thinkingContent}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function doCopy(text: string, setter: (v: boolean) => void) {
  navigator.clipboard.writeText(text);
  setter(true);
  setTimeout(() => setter(false), 2000);
}

function MessageItem({ message, onApply }: { message: ChatMessage; onApply: (code: string, suggested: string) => void }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [localCopied, setLocalCopied] = useState(false);
  const [userCopied, setUserCopied] = useState(false);
  const [toolMsgCopied, setToolMsgCopied] = useState(false);
  const [thinkingExpanded, setThinkingExpanded] = useState(false);
  const [thinkingCopied, setThinkingCopied] = useState(false);
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  const isTool = !!message.toolData;
  const hasThinking = !!message.thinkingContent;

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
      hasDetails ? t('chat.toolCalls.input', { input: JSON.stringify(toolInput) }) : '',
      toolResult ? t('chat.toolCalls.output', { output: toolResult }) : '',
    ].filter(Boolean).join('\n');

    if (isEspCli && toolResult) {
      return (
        <div className="flex flex-col items-center w-full max-w-[92%]">
          <div className="flex items-center gap-1 mb-1">
            <span className="text-[11px] text-text-tertiary">{label}</span>
            <button
              onClick={() => doCopy(toolMsgFullText, setToolMsgCopied)}
              className="px-1 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary rounded transition-all"
              title={t('common.copy')}
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
              title={t('chat.toolCalls.openFile', { filePath })}
            >
              <ExternalLink size={10} />
            </button>
          )}
          <button
            onClick={() => doCopy(toolMsgFullText, setToolMsgCopied)}
            className="px-1 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary rounded transition-all"
            title={t('chat.toolCalls.copyDetails')}
          >
            {toolMsgCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
          </button>
        </div>
        {expanded && (
          <div className="mt-1 px-3 py-2 bg-surface-root border border-border-subtle rounded-lg text-[11px] text-text-tertiary font-mono whitespace-pre-wrap max-h-[150px] overflow-y-auto max-w-[90%]">
            {hasDetails && <div className="text-text-secondary mb-1">{t('chat.toolCalls.inputLabel')}</div>}
            {hasDetails && <div>{JSON.stringify(toolInput, null, 2)}</div>}
            {toolResult && (
              <>
                <div className="text-text-secondary mt-2 mb-1">{t('chat.toolCalls.outputLabel')}</div>
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
            title={t('chat.copyMessage')}
          >
            {userCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={`flex flex-col group ${isUser ? 'items-end' : 'items-start'} animate-slide-up gap-2.5`}>
      {/* Avatar */}
      {isUser
        ? <div className="w-7 h-7 rounded-lg bg-accent flex items-center justify-center shrink-0"><User size={13} className="text-white" /></div>
        : <div className="h-7 w-28 rounded-lg flex items-center justify-center shrink-0 overflow-hidden bg-white/90 dark:bg-white/80 p-1">
            <img src={`/icons/${getCurrentToolchainId() === 'mimo' ? 'mimo-code' : getCurrentToolchainId()}.svg`} alt="" className="w-full h-full object-contain" />
          </div>
      }

      {/* Bubble — 按钮在左/右，内容在另一侧 */}
      <div className={`max-w-[80%] flex items-start flex-row gap-2`}>
        {/* 操作按钮 */}
        {isUser && (
          <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity shrink-0 pt-1">
          {message.id.startsWith('user-') && (
            <button
              onClick={() => useChatStore.getState().prepareRollback(message.id)}
              className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] text-text-tertiary hover:text-warning hover:bg-warning-muted rounded transition-all"
              title={t('chat.message.rollback')}
            >
              <Undo2 size={10} />
              {t('chat.message.rollbackBtn')}
            </button>
          )}
          {message.id.startsWith('user-') && (
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
              title={t('chat.message.reedit')}
            >
              <Pencil size={10} />
            </button>
          )}
          <button
            onClick={() => doCopy(message.content, setUserCopied)}
            className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary hover:bg-surface-hover rounded transition-all"
            title={t('chat.copyMessage')}
          >
            {userCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
          </button>
          </div>
        )}

        {/* 内容区 */}
        <div className={`min-w-0 ${isUser ? 'items-end' : 'items-start'}`}>
        {/* Thinking / Reasoning Block — 可折叠展示 AI 推理过程 */}
        {hasThinking && !isUser && (
          <div className="mb-2">
            <button
              onClick={() => setThinkingExpanded(!thinkingExpanded)}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[11px] text-text-tertiary hover:text-text-secondary hover:bg-surface-hover transition-all group/think w-full"
            >
              <Brain size={12} className="text-accent/70 shrink-0" />
              <span className="font-medium">{t('chat.thinking.process')}</span>
              <span className="text-text-disabled ml-1">{t('chat.thinking.charCount', { n: message.thinkingContent!.length > 0 ? Math.round(message.thinkingContent!.length / 2) : '...' })}</span>
              {thinkingExpanded
                ? <ChevronDown size={12} className="ml-auto shrink-0" />
                : <ChevronRight size={12} className="ml-auto shrink-0" />
              }
              <button
                onClick={(e) => { e.stopPropagation(); doCopy(message.thinkingContent!, setThinkingCopied); }}
                className="opacity-0 group-hover/think:opacity-100 transition-opacity shrink-0"
                title={t('chat.thinking.copy')}
              >
                {thinkingCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
              </button>
            </button>
            {thinkingExpanded && (
              <div className="mt-1 mx-3 px-3 py-2.5 bg-[var(--color-thinking-bg,#1a1625)] border border-border-subtle rounded-lg text-[12px] text-text-tertiary leading-relaxed max-h-[300px] overflow-y-auto">
                <div className="[&_p]:mb-2 [&_p:last-of-type]:mb-0 [&_ul]:list-disc [&_ul]:pl-4 [&_ol]:list-decimal [&_ol]:pl-4 whitespace-pre-wrap">
                  {message.thinkingContent}
                </div>
              </div>
            )}
          </div>
        )}
        <div className={`px-4 py-2.5 text-[13px] leading-relaxed ${
          isUser
            ? 'bg-accent text-white rounded-2xl rounded-tr-sm'
            : 'bg-surface-overlay border border-border-subtle rounded-2xl rounded-tl-sm'
        }`}>
          <MessageContent content={message.content} onApply={onApply} />
        </div>
        {!isUser && message.content && (
          <div className="flex items-center gap-1 mt-1">
            <button
              onClick={() => doCopy(message.content, setLocalCopied)}
              className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] text-text-tertiary hover:text-text-primary hover:bg-surface-hover rounded transition-all"
              title={t('chat.copyMessage')}
            >
              {localCopied ? <Check size={10} className="text-success" /> : <Copy size={10} />}
              {t('common.copy')}
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
    </div>
  );
}

function CodeBlock({ language, code, content, onApply }: { language: string; code: string; content: string; onApply: (code: string, suggested: string) => void }) {
  const { t } = useTranslation();
  const [copyOk, setCopyOk] = useState(false);

  const extractFilePath = (): string => {
    const idx = content.indexOf(code);
    if (idx === -1) return '';
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
    return '';
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(code);
    setCopyOk(true);
    setTimeout(() => setCopyOk(false), 2000);
  };

  const handleApply = () => {
    onApply(code, extractFilePath());
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
            className="flex items-center gap-1 px-2 py-0.5 text-[10px] bg-accent text-white rounded hover:bg-accent-hover transition-colors"
            onClick={handleApply}
          >
            {t('chat.codeBlock.apply')}
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

function MessageContent({ content, onApply }: { content: string; onApply: (code: string, suggested: string) => void }) {
  return (
    <div className="[&_p]:mb-2 [&_p:last-of-type]:mb-0 [&_ul]:list-disc [&_ul]:pl-5 [&_ul]:mb-2 [&_ul]:space-y-1 [&_ol]:list-decimal [&_ol]:pl-5 [&_ol]:mb-2 [&_ol]:space-y-1 [&_h1]:text-lg [&_h1]:font-bold [&_h1]:mb-2 [&_h2]:text-base [&_h2]:font-bold [&_h2]:mb-2 [&_h3]:text-sm [&_h3]:font-bold [&_h3]:mb-1 [&_table]:border-collapse [&_table]:mb-2 [&_table]:text-[12px] [&_th]:border [&_th]:border-border-subtle [&_th]:px-2 [&_th]:py-1 [&_th]:bg-surface-overlay [&_th]:font-medium [&_td]:border [&_td]:border-border-subtle [&_td]:px-2 [&_td]:py-1 [&_blockquote]:border-l-2 [&_blockquote]:border-accent [&_blockquote]:pl-3 [&_blockquote]:italic [&_blockquote]:text-text-tertiary [&_blockquote]:mb-2 [&_a]:text-accent [&_a]:hover:underline [&_hr]:border-border-subtle [&_hr]:my-2">
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkBreaks]}
      components={{
        code({ node, className, children, ...props }) {
          const match = /language-(\w+)/.exec(className || '');
          const codeString = String(children).replace(/\n$/, '');
          if (match) {
            return <CodeBlock language={match[1]} code={codeString} content={content} onApply={onApply} />;
          }
          return <code className="px-1 py-0.5 rounded bg-surface-overlay text-text-secondary font-mono text-[12px]" {...props}>{children}</code>;
        },
        a({ node, href, children, ...props }) {
          return (
            <a
              href={href}
              onClick={(e) => {
                e.preventDefault();
                if (href) open(href);
              }}
              className="text-accent hover:underline cursor-pointer"
              {...props}
            >
              {children}
            </a>
          );
        },
      }}
    >
      {content}
    </ReactMarkdown>
    </div>
  );
}

const ChatPanelMemo = memo(ChatPanel);
export { ChatPanelMemo as ChatPanel };
export default ChatPanelMemo;