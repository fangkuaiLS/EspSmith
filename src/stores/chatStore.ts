/**
 * AI 聊天状态管理
 *
 * 使用统一 API 层，自动适配 Tauri / 浏览器 / Demo 三种模式
 */

import { create } from 'zustand';
import { sendChatMessage, startAI, stopAI, clearConversation } from '../lib/api';
import { useProjectStore } from './projectStore';
import { useFileStore } from './fileStore';
import { useHardwareStore } from './hardwareStore';
import { safeInvoke } from '../lib/invoke';
import { translateBackendString } from '../i18n';
import type { AICumulativeUsage } from '../types/chat';

// ==================== 会话持久化 ====================

export interface ChatSession {
    id: string;
    title: string;
    messages: ChatMessage[];
    createdAt: number;
    updatedAt: number;
}

const SESSIONS_KEY_PREFIX = 'esp-ai-sessions:';

function getSessionsKey(): string {
    const project = useProjectStore.getState().currentProject;
    return `${SESSIONS_KEY_PREFIX}${project?.path || 'global'}`;
}

function loadSessionsFromStorage(): ChatSession[] {
    try {
        const raw = localStorage.getItem(getSessionsKey());
        return raw ? JSON.parse(raw) : [];
    } catch {
        return [];
    }
}

function saveSessionsToStorage(sessions: ChatSession[]): void {
    try {
        localStorage.setItem(getSessionsKey(), JSON.stringify(sessions.slice(0, 50)));
    } catch { /* ignore */ }
}

export type AIStatus = 'idle' | 'thinking' | 'building' | 'flashing' | 'tool_call' | 'error';

export interface OperationStep {
    label: string;
    status: string;
}

export interface OperationProgress {
    operationId: string;
    /**
     * CodeWhale `tool_use.id` that originated this op. Used to verify
     * ownership on ai-operation-done so a stray tool_result (e.g. for a
     * non-JTAG tool) cannot prematurely mark the card as done.
     */
    toolUseId?: string;
    operationType: string;
    label: string;
    steps: OperationStep[];
    command: string;
    startedAt?: number;
    currentStepIndex?: number;
}

export interface ToolData {
    name: string;
    input: unknown;
    result?: string;
}

export interface ChatMessage {
    id: string;
    role: 'user' | 'assistant' | 'system';
    content: string;
    timestamp: number;
    status?: AIStatus;
    toolData?: ToolData;
    usage?: { inputTokens: number; outputTokens: number; cachedTokens: number; totalTokens: number; costRmb: number };
    /** AI 推理/思考过程内容（独立于正式回复，可折叠展示） */
    thinkingContent?: string;
}

export interface RollbackInfo {
    userMessageId: string;
    restoreFiles: Array<{ path: string; action: 'restore' }>;
    deleteFiles: Array<{ path: string; action: 'delete' }>;
}

// 模块级 AbortController，用于打断正在进行的 AI 请求
let currentAbortController: AbortController | null = null;

// 任务完成声音提示
function playCompleteSound() {
    try {
        const ctx = new AudioContext();
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.type = 'sine';
        osc.frequency.setValueAtTime(660, ctx.currentTime);
        osc.frequency.setValueAtTime(880, ctx.currentTime + 0.08);
        gain.gain.setValueAtTime(0.08, ctx.currentTime);
        gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.25);
        osc.start(ctx.currentTime);
        osc.stop(ctx.currentTime + 0.25);
    } catch {
        // 浏览器可能禁止自动播放音频，忽略
    }
}

// 将工具路径解析为绝对路径
function resolveToolPath(rawPath: string | undefined): string | null {
    if (!rawPath) return null;
    if (/^[A-Z]:[\\/]/i.test(rawPath) || rawPath.startsWith('/')) return rawPath;
    const project = useProjectStore.getState().currentProject;
    if (!project?.path) return rawPath;
    return `${project.path}\\${rawPath}`;
}

function normalizeToolName(name: string): string {
    return name.split(/[.:]/).pop()?.replace(/^mcp__[^_]+__/, '') || name;
}

function getToolInputPath(input: unknown): string | undefined {
    const inp = input as Record<string, unknown> | undefined;
    const args = inp?.arguments as Record<string, unknown> | undefined;
    return (inp?.path || inp?.file_path || inp?.filePath || args?.path || args?.file_path || args?.filePath) as string | undefined;
}

// 工具名称 → 人类可读标签
function getToolLabel(name: string, input: unknown): string {
    const inp = input as Record<string, unknown> | undefined;
    switch (name) {
        case 'list_dir':
            return `列出目录: \`${inp?.path || '.'}\``;
        case 'read_file':
            return `读取文件: \`${inp?.path || inp?.file_path || '...'}\``;
        case 'write_file':
            return `写入文件: \`${inp?.path || inp?.file_path || '...'}\``;
        case 'exec_shell':
            return `执行命令: \`${String(inp?.command || '').slice(0, 80)}\``;
        case 'build_project':
            return 'Build ESP-IDF project';
        case 'flash_project':
            return `Flash firmware: \`${inp?.port || 'port'}\``;
        case 'build_flash_monitor':
            return `Build + flash + monitor: \`${inp?.port || 'port'}\``;
        case 'closed_loop':
            return `一键闭环: 编译→烧录→验证 (\`${inp?.port || 'port'}\`)`;
        case 'list_serial_ports':
            return 'List serial ports';
        case 'read_serial':
            return `Read serial: \`${inp?.port || 'port'}\``;
        case 'run_gdb_command':
            return `Run GDB: \`${String(inp?.command || '').slice(0, 80)}\``;
        case 'get_hardware_config':
            return 'Read hardware config';
        case 'export_hardware_header':
            return 'Generate hardware_config.h';
        case 'search':
            return `搜索: \`${inp?.query || inp?.pattern || '...'}\``;
        default:
            return `调用工具: ${name}`;
    }
}

// ==================== Checkpoint 系统 ====================

interface FileSnapshot {
    filePath: string;
    content: string | null;
}

const checkpoints = new Map<string, FileSnapshot[]>();
// 追踪每条用户消息中 AI 实际修改的文件（用于回退确认显示）
const touchedFilesByMessage = new Map<string, Set<string>>();

async function captureCheckpoint(userMessageId: string): Promise<void> {
    const project = useProjectStore.getState().currentProject;
    if (!project?.path) return;

    const filePaths = await collectProjectFilePaths(project.path);

    const snapshots: FileSnapshot[] = [];

    for (const filePath of filePaths) {
        try {
            const content = await safeInvoke<string>('read_file', { path: filePath });
            if (content !== null) {
                snapshots.push({ filePath, content });
            }
        } catch {
            // 跳过二进制文件或无法读取的文件
        }
    }

    checkpoints.set(userMessageId, snapshots);
}

async function collectProjectFilePaths(rootPath: string): Promise<Set<string>> {
    const out = new Set<string>();
    const SKIP_DIRS = new Set(['.git', 'build', 'node_modules', 'target', 'dist', '.vscode', '.idea']);
    const SKIP_EXTS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'ico', 'svgz', 'ttf', 'otf', 'woff', 'woff2', 'bdf', 'fon',
        'exe', 'dll', 'so', 'dylib', 'bin', 'obj', 'o', 'a', 'lib',
        'zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar',
        'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx',
        'pyc', 'pyo', 'class', 'elf', 'wasm',
        'mp3', 'wav', 'ogg', 'flac', 'mp4', 'avi', 'mkv', 'webm',
        'dat', 'dmg', 'iso', 'odg', 'xcf', 'psd', 'ai', 'eps']);
    const visit = async (dir: string) => {
        const entries = await safeInvoke<Array<{ path: string; is_dir: boolean }>>('list_directory', { path: dir });
        for (const entry of entries || []) {
            if (entry.is_dir) {
                const name = entry.path.split(/[\\/]/).pop()?.toLowerCase();
                if (!name || SKIP_DIRS.has(name) || name.startsWith('%') || name.startsWith('$')) {
                    continue;
                }
                await visit(entry.path);
            } else {
                const ext = entry.path.split('.').pop()?.toLowerCase();
                if (ext && SKIP_EXTS.has(ext)) {
                    continue;
                }
                out.add(entry.path);
            }
        }
    };
    await visit(rootPath);
    return out;
}

// ==================== Store ====================

interface ChatStore {
    messages: ChatMessage[];
    status: AIStatus;
    isRunning: boolean;
    pendingRollback: RollbackInfo | null;
    usage: AICumulativeUsage | null;
    sessions: ChatSession[];
    activeSessionId: string | null;
    permissionMode: 'full' | 'ask';
    pendingPermission: { toolName: string; reason: string } | null;
    activeOperation: OperationProgress | null;
    /** 等待发送的消息队列（当前任务执行期间用户提交的新消息） */
    messageQueue: string[];

    sendMessage: (content: string) => Promise<void>;
    startAI: () => Promise<void>;
    stopAI: () => Promise<void>;
    updateStatus: (status: AIStatus) => void;
    setActiveOperation: (op: OperationProgress | null) => void;
    clearMessages: (saveSession?: boolean) => void;
    addMessage: (message: ChatMessage) => void;
    appendToLastAssistant: (chunk: string) => void;
    appendToLastThinking: (chunk: string) => void;
    setLastAssistantContent: (content: string) => void;
    updateToolMessage: (id: string, result: string) => void;
    prepareRollback: (userMessageId: string) => Promise<void>;
    confirmRollback: () => Promise<void>;
    cancelRollback: () => void;
    restoreMessages: (messages: ChatMessage[]) => void;
    resetMessages: () => void;
    loadSessions: () => void;
    saveCurrentSession: () => void;
    loadSession: (id: string) => void;
    deleteSession: (id: string) => void;
    setPermissionMode: (mode: 'full' | 'ask') => Promise<void>;
    loadPermissionMode: () => Promise<void>;
    respondPermission: (allow: boolean) => Promise<void>;
    /** 将消息加入等待队列 */
    enqueueMessage: (content: string) => void;
    /** 清空等待队列 */
    clearQueue: () => void;
}

export const useChatStore = create<ChatStore>((set, get) => ({
    messages: [],
    status: 'idle',
    isRunning: false,
    pendingRollback: null,
    usage: null,
    sessions: [],
    activeSessionId: null,
    permissionMode: 'full',
    pendingPermission: null,
    activeOperation: null,
    messageQueue: [],

    sendMessage: async (content: string) => {
        const { addMessage, updateStatus, appendToLastAssistant, appendToLastThinking, setLastAssistantContent, updateToolMessage } = get();

        currentAbortController = new AbortController();
        const signal = currentAbortController.signal;

        const userMessageId = `user-${Date.now()}`;
        addMessage({
            id: userMessageId,
            role: 'user',
            content,
            timestamp: Date.now(),
        });

        const assistantId = `assistant-${Date.now()}`;
        addMessage({
            id: assistantId,
            role: 'assistant',
            content: '',
            timestamp: Date.now(),
        });

        updateStatus('thinking');

        await captureCheckpoint(userMessageId);

        let unlistenToolUse: (() => void) | undefined;
        let unlistenToolResult: (() => void) | undefined;
        let unlistenUsage: (() => void) | undefined;
        let unlistenOpProgress: (() => void) | undefined;
        let unlistenOpDone: (() => void) | undefined;
        let unlistenReasoning: (() => void) | undefined;
        // Pending "activeOperation → null" clear. Cancellable so that a new
        // ai-operation-progress arriving within 3s (or the user stopping
        // the run) does not wipe a freshly-started op.
        let opDoneTimer: ReturnType<typeof setTimeout> | undefined;
        const cancelOpDoneTimer = () => {
            if (opDoneTimer !== undefined) {
                clearTimeout(opDoneTimer);
                opDoneTimer = undefined;
            }
        };
        const touchedFiles = new Set<string>();
        const toolMsgMap = new Map<string, string>();
        // 统计工具调用次数，用于 AI 完成后的总结
        const toolCounts = { read: 0, write: 0, exec: 0, list: 0, delete: 0 };

        if (typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)) {
            const { listen } = await import('@tauri-apps/api/event');
            unlistenToolUse = await listen<{ name: string; id: string; input: unknown }>('ai-tool-use', (event) => {
                if (signal.aborted) return;
                const toolName = normalizeToolName(event.payload.name);
                // 根据工具类型设置不同状态，显示内联 UI 提示
                if (toolName === 'build_project' || toolName === 'build_flash_monitor' || toolName === 'closed_loop') {
                    updateStatus('building');
                } else if (toolName === 'flash_project') {
                    updateStatus('flashing');
                } else {
                    updateStatus('tool_call');
                }
                const toolMsgId = `tool-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
                toolMsgMap.set(String(event.payload.id), toolMsgId);
                addMessage({
                    id: toolMsgId,
                    role: 'system',
                    content: `🔧 ${getToolLabel(event.payload.name, event.payload.input)}`,
                    timestamp: Date.now(),
                    toolData: { name: event.payload.name, input: event.payload.input },
                });
                // 统计工具调用
                switch (toolName) {
                    case 'read_file': toolCounts.read++; break;
                    case 'write_file': toolCounts.write++; break;
                    case 'exec_shell': toolCounts.exec++; break;
                    case 'build_project':
                    case 'flash_project':
                    case 'build_flash_monitor':
                    case 'closed_loop':
                    case 'read_serial':
                    case 'run_gdb_command':
                        toolCounts.exec++;
                        break;
                    case 'list_dir': toolCounts.list++; break;
                    case 'list_directory':
                    case 'list_serial_ports':
                        toolCounts.list++;
                        break;
                }
                // codewhale 可能用 delete_file 或 exec_shell rm/del 来删除文件
                if (toolName === 'delete_file' || toolName === 'remove_file') {
                    toolCounts.delete++;
                }
                if (toolName === 'exec_shell') {
                    const inp = event.payload.input as Record<string, unknown> | undefined;
                    const cmd = String(inp?.command || '').toLowerCase();
                    if (/\b(rm|del|remove)\b/.test(cmd)) {
                        toolCounts.delete++;
                    }
                }
                if (toolName === 'write_file') {
                    const absPath = resolveToolPath(getToolInputPath(event.payload.input));
                    if (absPath) {
                        touchedFiles.add(absPath);
                        // 记录到 message 级别的追踪表
                        let msgFiles = touchedFilesByMessage.get(userMessageId);
                        if (!msgFiles) {
                            msgFiles = new Set();
                            touchedFilesByMessage.set(userMessageId, msgFiles);
                        }
                        msgFiles.add(absPath);
                    }
                }
            });
            unlistenToolResult = await listen<{ id: string; status: string; output?: string }>('ai-tool-result', (event) => {
                if (signal.aborted) return;
                updateStatus('thinking');
                const toolMsgId = toolMsgMap.get(String(event.payload.id));
                if (toolMsgId && event.payload.output) {
                    const truncated = event.payload.output.length > 3000
                        ? event.payload.output.slice(0, 3000) + '\n... (结果已截断)'
                        : event.payload.output;
                    updateToolMessage(toolMsgId, truncated);
                }
            });
            unlistenUsage = await listen<AICumulativeUsage>('ai-usage', (event) => {
                if (signal.aborted) return;
                const payload = event.payload;
                set({ usage: payload });
                if (payload.lastMessage && payload.lastMessage.totalTokens > 0) {
                    set((state) => {
                        const msgs = [...state.messages];
                        for (let i = msgs.length - 1; i >= 0; i--) {
                            if (msgs[i].role === 'assistant' && !msgs[i].toolData) {
                                msgs[i] = { ...msgs[i], usage: payload.lastMessage };
                                break;
                            }
                        }
                        return { messages: msgs };
                    });
                }
            });
            unlistenOpProgress = await listen<OperationProgress>('ai-operation-progress', (event) => {
                if (signal.aborted) return;
                const op = event.payload;
                // A new (or refreshed) progress payload means the operation
                // timeline must keep running. Drop any pending "done → null"
                // clear from a previous op so we don't blank it out 3s later.
                cancelOpDoneTimer();
                const existing = get().activeOperation;
                if (existing && existing.operationId === op.operationId) {
                    op.startedAt = existing.startedAt;
                } else {
                    op.startedAt = Date.now();
                }
                // 从步骤状态推断 currentStepIndex，而非硬编码为 0
                const runningIdx = op.steps.findIndex(s => s.status === 'running');
                op.currentStepIndex = runningIdx >= 0 ? runningIdx : op.steps.filter(s => s.status === 'done').length;
                set({ activeOperation: op });
            });
            unlistenOpDone = await listen<{ toolUseId: string; status?: string }>('ai-operation-done', (event) => {
                if (signal.aborted) return;
                const toolUseId = event.payload?.toolUseId;
                const current = get().activeOperation;
                if (!current) return;
                if (toolUseId && current.toolUseId && current.toolUseId !== toolUseId) {
                    return;
                }
                cancelOpDoneTimer();
                // Check if the operation succeeded or failed
                const opStatus = event.payload?.status || '';
                const isError = opStatus === 'error' || opStatus.includes('error') || opStatus.includes('fail');
                // 只标记尚未完成（pending/running）的步骤，保留已有状态
                // 避免覆盖后端已逐步推送的真实进度
                const finishedSteps = current.steps.map(s => ({
                    ...s,
                    status: s.status === 'done' || s.status === 'error' ? s.status : (isError ? 'error' : 'done'),
                }));
                set({ activeOperation: { ...current, steps: finishedSteps } });
                opDoneTimer = setTimeout(() => {
                    opDoneTimer = undefined;
                    if (!signal.aborted) set({ activeOperation: null });
                }, 3000);
            });
            unlistenReasoning = await listen<string>('ai-reasoning', (event) => {
                if (signal.aborted) return;
                console.log('[Thinking] reasoning event received:', event.payload?.slice(0, 100));
                appendToLastThinking(event.payload);
            });
        }

        try {
            const finalResponse = await sendChatMessage(content, (chunk) => {
                if (!signal.aborted) {
                    appendToLastAssistant(chunk);
                }
            });

            if (signal.aborted) return;

            const state = get();
            // Find the last assistant message (tool messages may come after it)
            const lastAssistant = [...state.messages].reverse().find(m => m.role === 'assistant');
            if (lastAssistant && lastAssistant.id === assistantId) {
                setLastAssistantContent(finalResponse);
            }

            updateStatus('idle');
            set({ activeOperation: null });

            playCompleteSound();
            // 生成操作总结
            const summaryParts: string[] = [];
            if (toolCounts.read > 0) summaryParts.push(`读取了 ${toolCounts.read} 个文件`);
            if (toolCounts.write > 0) summaryParts.push(`修改了 ${toolCounts.write} 个文件`);
            if (toolCounts.delete > 0) summaryParts.push(`删除了 ${toolCounts.delete} 个文件`);
            if (toolCounts.exec > 0) summaryParts.push(`执行了 ${toolCounts.exec} 个命令`);
            if (toolCounts.list > 0) summaryParts.push(`浏览了 ${toolCounts.list} 个目录`);
            const summaryContent = summaryParts.length > 0
                ? `✅ 任务完成：${summaryParts.join('，')}`
                : '✅ 任务完成';
            addMessage({
                id: `complete-${Date.now()}`,
                role: 'system',
                content: summaryContent,
                timestamp: Date.now(),
            });
        } catch (error) {
            if (signal.aborted) return;
            console.error('Chat error:', error);
            appendToLastAssistant(`\n\n❌ 请求失败: ${translateBackendString(String(error))}`);
            updateStatus('error');
        } finally {
            cancelOpDoneTimer();
            currentAbortController = null;
            unlistenToolUse?.();
            unlistenToolResult?.();
            unlistenUsage?.();
            unlistenOpProgress?.();
            unlistenOpDone?.();
            unlistenReasoning?.();
            const currentStatus = get().status;
            if (currentStatus === 'thinking' || currentStatus === 'tool_call') {
                updateStatus('idle');
            }
            const project = useProjectStore.getState().currentProject;
            if (project?.path) {
                await useFileStore.getState().loadDirectory(project.path);
                await useFileStore.getState().refreshOpenTabs();

                const before = new Set((checkpoints.get(userMessageId) || []).map((s) => s.filePath));
                const after = await collectProjectFilePaths(project.path);
                const LIBRARY_DIRS = ['managed_components', '.espressif', 'dependencies.lock', 'sdkconfig', 'sdkconfig.old'];
                for (const filePath of after) {
                    if (!before.has(filePath)) {
                        const rel = filePath.replace(project.path, '').replace(/^[\\/]/, '');
                        const isLibrary = LIBRARY_DIRS.some(d => rel.startsWith(d) || rel.includes(`\\${d}\\`) || rel.includes(`/${d}/`));
                        if (!isLibrary) {
                            touchedFiles.add(filePath);
                        }
                    }
                }
                const hasHardwarePinChange = [...touchedFiles].some(f => f.endsWith('hardware_pins.h'));
                if (hasHardwarePinChange) {
                    await useHardwareStore.getState().loadConfig(project.path);
                }
            }
            for (const filePath of touchedFiles) {
                useFileStore.getState().openFile(filePath).catch(() => {});
            }

            // 队列处理：当前任务正常完成（非用户主动停止）后，自动发送队列中的下一条消息
            if (!signal.aborted) {
                const queue = get().messageQueue;
                if (queue.length > 0) {
                    const nextMessage = queue[0];
                    set({ messageQueue: queue.slice(1) });
                    // 延迟执行，确保当前 finally 完全结束后再开始下一轮
                    setTimeout(() => {
                        get().sendMessage(nextMessage);
                    }, 0);
                }
            }
        }
    },

    startAI: async () => {
        try {
            await startAI();
            set({ isRunning: true, status: 'idle' });
        } catch (err) {
            set({ isRunning: false, status: 'error' });
            set((state) => ({
                messages: [...state.messages, {
                    id: `sys-error-${Date.now()}`,
                    role: 'system' as const,
                    content: `AI 启动失败: ${translateBackendString(err instanceof Error ? err.message : String(err))}`,
                    timestamp: Date.now(),
                }],
            }));
        }
    },

    stopAI: async () => {
        if (currentAbortController) {
            currentAbortController.abort();
        }
        try {
            await stopAI();
        } catch { /* 忽略 */ }
        set({ isRunning: false, status: 'idle', activeOperation: null, messageQueue: [] });
    },

    updateStatus: (status) => set({ status }),

    setActiveOperation: (op) => set({ activeOperation: op }),

    clearMessages: (saveSession = false) => {
        if (saveSession) {
            const state = get();
            const userMessages = state.messages.filter(m => m.role === 'user' && m.content.trim());
            if (userMessages.length > 0) {
                const title = userMessages[0].content.slice(0, 60);
                const session: ChatSession = {
                    id: state.activeSessionId || `session-${Date.now()}`,
                    title,
                    messages: state.messages.filter(m => m.role !== 'system'),
                    createdAt: Date.now(),
                    updatedAt: Date.now(),
                };
                const sessions = loadSessionsFromStorage();
                const idx = sessions.findIndex(s => s.id === session.id);
                if (idx >= 0) {
                    sessions[idx] = session;
                } else {
                    sessions.unshift(session);
                }
                saveSessionsToStorage(sessions);
                set({ sessions });
            }
        }
        clearConversation();
        set({ messages: [], usage: null, activeSessionId: `session-${Date.now()}`, messageQueue: [] });
    },

    addMessage: (message) => {
        set((state) => ({ messages: [...state.messages, message] }));
    },

    appendToLastAssistant: (chunk: string) => {
        set((state) => {
            const messages = [...state.messages];
            // Search backwards — tool messages may be inserted after the assistant message
            for (let i = messages.length - 1; i >= 0; i--) {
                if (messages[i].role === 'assistant') {
                    messages[i] = {
                        ...messages[i],
                        content: messages[i].content + chunk,
                    };
                    break;
                }
            }
            return { messages };
        });
    },

    appendToLastThinking: (chunk: string) => {
        set((state) => {
            const messages = [...state.messages];
            for (let i = messages.length - 1; i >= 0; i--) {
                if (messages[i].role === 'assistant') {
                    const prevThinking = messages[i].thinkingContent || '';
                    messages[i] = {
                        ...messages[i],
                        thinkingContent: prevThinking + chunk,
                    };
                    break;
                }
            }
            return { messages };
        });
    },

    setLastAssistantContent: (content: string) => {
        set((state) => {
            const messages = [...state.messages];
            // Search backwards — tool messages may be inserted after the assistant message
            for (let i = messages.length - 1; i >= 0; i--) {
                if (messages[i].role === 'assistant') {
                    messages[i] = { ...messages[i], content };
                    break;
                }
            }
            return { messages };
        });
    },

    updateToolMessage: (id: string, result: string) => {
        set((state) => ({
            messages: state.messages.map((m) =>
                m.id === id && m.toolData
                    ? { ...m, toolData: { ...m.toolData, result } }
                    : m
            ),
        }));
    },

    // 准备回退：计算文件变更并显示确认对话框
    prepareRollback: async (userMessageId: string) => {
        const snaps = checkpoints.get(userMessageId);
        if (!snaps) return;

        const snapshotPaths = new Set(snaps.map(s => s.filePath));
        const project = useProjectStore.getState().currentProject;
        if (!project?.path) return;
        const currentPaths = await collectProjectFilePaths(project.path);
        // 优先使用实际追踪到的修改文件，没追踪才回退到快照内容比对
        const trackedFiles = touchedFilesByMessage.get(userMessageId);

        const restoreFiles: RollbackInfo['restoreFiles'] = [];
        const deleteFiles: RollbackInfo['deleteFiles'] = [];

        if (trackedFiles && trackedFiles.size > 0) {
            // 有追踪数据：直接用追踪到的文件列表
            for (const filePath of trackedFiles) {
                if (currentPaths.has(filePath)) {
                    restoreFiles.push({ path: filePath, action: 'restore' });
                }
            }
        } else {
            // 无追踪数据（老消息）：对比快照内容与当前磁盘内容
            const { invoke } = await import('@tauri-apps/api/core');
            for (const snap of snaps) {
                if (snap.content !== null && currentPaths.has(snap.filePath)) {
                    try {
                        const currentContent = await invoke<string>('read_file', { path: snap.filePath });
                        if (currentContent !== snap.content) {
                            restoreFiles.push({ path: snap.filePath, action: 'restore' });
                        }
                    } catch { /* skip */ }
                }
            }
        }

        // 当前文件树中存在但快照中没有的文件 = AI 新建的
        for (const currentPath of currentPaths) {
            if (!snapshotPaths.has(currentPath)) {
                deleteFiles.push({ path: currentPath, action: 'delete' });
            }
        }

        if (restoreFiles.length === 0 && deleteFiles.length === 0) {
            return; // 没有变更，无需回退
        }

        set({ pendingRollback: { userMessageId, restoreFiles, deleteFiles } });
    },

    // 确认回退
    confirmRollback: async () => {
        const { pendingRollback } = get();
        if (!pendingRollback) return;

        const snaps = checkpoints.get(pendingRollback.userMessageId);
        const { invoke } = await import('@tauri-apps/api/core');

        // 恢复被修改的文件
        if (snaps) {
            for (const snap of snaps) {
                if (snap.content !== null) {
                    try {
                        await invoke('write_file', { path: snap.filePath, content: snap.content, safeMode: false });
                    } catch { /* 忽略 */ }
                }
            }
        }

        // 删除 AI 新建的文件
        for (const file of pendingRollback.deleteFiles) {
            try {
                await invoke('delete_file', { path: file.path });
            } catch { /* 忽略 */ }
        }

        // 刷新文件树和标签页
        const project = useProjectStore.getState().currentProject;
        if (project?.path) {
            useFileStore.getState().loadDirectory(project.path);
            useFileStore.getState().refreshOpenTabs();
        }

        // 对话内容回退到对应节点
        const targetId = pendingRollback.userMessageId;
        set((state) => {
            const idx = state.messages.findIndex((m) => m.id === targetId);
            if (idx === -1) return { pendingRollback: null };
            // 保留该用户消息之前的所有消息，移除该消息及之后的一切
            return {
                pendingRollback: null,
                messages: state.messages.slice(0, idx),
            };
        });
    },

    // 取消回退
    cancelRollback: () => {
        set({ pendingRollback: null });
    },

    // 恢复聊天消息（项目缓存恢复用）
    restoreMessages: (messages: ChatMessage[]) => {
        set({ messages });
    },

    // 重置为默认欢迎消息（切换项目时调用）
    resetMessages: () => {
        set({ messages: [] });
    },

    loadSessions: () => {
        const sessions = loadSessionsFromStorage();
        set({ sessions });
    },

    saveCurrentSession: () => {
        const state = get();
        const userMessages = state.messages.filter(m => m.role === 'user' && m.content.trim());
        if (userMessages.length === 0) return;
        const title = userMessages[0].content.slice(0, 60);
        const session: ChatSession = {
            id: state.activeSessionId || `session-${Date.now()}`,
            title,
            messages: state.messages.filter(m => m.role !== 'system'),
            createdAt: Date.now(),
            updatedAt: Date.now(),
        };
        const sessions = loadSessionsFromStorage();
        const idx = sessions.findIndex(s => s.id === session.id);
        if (idx >= 0) {
            sessions[idx] = session;
        } else {
            sessions.unshift(session);
        }
        saveSessionsToStorage(sessions);
        set({ sessions });
    },

    loadSession: (id: string) => {
        const state = get();
        const currentUserMessages = state.messages.filter(m => m.role === 'user' && m.content.trim());
        if (currentUserMessages.length > 0 && state.activeSessionId && state.activeSessionId !== id) {
            const title = currentUserMessages[0].content.slice(0, 60);
            const session: ChatSession = {
                id: state.activeSessionId,
                title,
                messages: state.messages.filter(m => m.role !== 'system'),
                createdAt: Date.now(),
                updatedAt: Date.now(),
            };
            const sessions = loadSessionsFromStorage();
            const idx = sessions.findIndex(s => s.id === session.id);
            if (idx >= 0) {
                sessions[idx] = session;
            } else {
                sessions.unshift(session);
            }
            saveSessionsToStorage(sessions);
        }

        const sessions = loadSessionsFromStorage();
        const session = sessions.find(s => s.id === id);
        if (!session) return;
        if (currentAbortController) {
            currentAbortController.abort();
        }
        stopAI().catch(() => {});
        set({
            messages: session.messages,
            activeSessionId: session.id,
            isRunning: false,
            status: 'idle',
            usage: null,
            sessions,
            messageQueue: [],
        });
        startAI().catch(() => {});
    },

    deleteSession: (id: string) => {
        const sessions = loadSessionsFromStorage().filter(s => s.id !== id);
        saveSessionsToStorage(sessions);
        const isActive = get().activeSessionId === id;
        set({
            sessions,
            ...(isActive ? { activeSessionId: `session-${Date.now()}`, messages: [], usage: null } : {}),
        });
    },

    setPermissionMode: async (mode: 'full' | 'ask') => {
        await safeInvoke('ai_set_permission_mode', { mode });
        set({ permissionMode: mode });
    },

    loadPermissionMode: async () => {
        try {
            const mode = await safeInvoke('ai_get_permission_mode', {}) as string;
            set({ permissionMode: (mode === 'ask' ? 'ask' : 'full') });
        } catch {
            set({ permissionMode: 'full' });
        }
    },

    respondPermission: async (allow: boolean) => {
        await safeInvoke('ai_respond_permission', { allow });
        set({ pendingPermission: null });
    },

    enqueueMessage: (content: string) => {
        set((state) => ({ messageQueue: [...state.messageQueue, content] }));
    },

    clearQueue: () => {
        set({ messageQueue: [] });
    },
}));
