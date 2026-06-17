/**
 * 统一 API 层 — CodeWhale Agent 深度集成
 *
 * Tauri 桌面模式：通过 invoke 调用 Rust 后端，Rust spawn codewhale exec 进程。
 * 流式输出通过 Tauri events（ai-chunk）实时推送。
 * 浏览器模式：显示提示信息，完整 AI 功能需在桌面应用中体验。
 */

import { useSettingsStore } from '../stores/settingsStore';
import { useProjectStore } from '../stores/projectStore';
import { translateBackendString } from '../i18n';

// ==================== 环境检测 ====================

function isTauri(): boolean {
    return typeof window !== 'undefined' &&
        ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);
}

// ==================== 统一接口 ====================

export async function sendChatMessage(
    content: string,
    onChunk?: (text: string) => void,
): Promise<string> {
    if (isTauri()) {
        const { invoke } = await import('@tauri-apps/api/core');
        const { listen } = await import('@tauri-apps/api/event');

        // 先注册事件监听，再调用 invoke，确保不丢 chunk
        const unlisten = await listen<string>('ai-chunk', (event) => {
            if (onChunk) onChunk(translateBackendString(event.payload));
        });

        try {
            const response = await invoke<string>('ai_send_message', { message: content });
            return response;
        } finally {
            unlisten();
        }
    }

    return '请在 EspSmith 桌面应用中体验完整 AI 功能。\n\n运行 `npm run tauri dev` 启动桌面应用。';
}

export async function clearConversation() {
    if (isTauri()) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('ai_clear_session').catch(() => {});
    }
}

/**
 * 恢复后端会话 ID — 加载历史会话时调用，让 AI 继续之前的上下文
 */
export async function setSessionId(sessionId: string): Promise<void> {
    if (isTauri()) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('ai_set_session_id', { sessionId });
    }
}

export async function startAI(): Promise<void> {
    if (isTauri()) {
        const { settings, idfStatus } = useSettingsStore.getState();
        const project = useProjectStore.getState().currentProject;
        const { invoke } = await import('@tauri-apps/api/core');

        let model: string = settings.deepseekModel || 'deepseek-v4-flash';
        let apiKey = settings.deepseekApiKey || null;

        if (settings.aiModel === 'ollama') {
            model = 'ollama';
        } else if (settings.aiModel === 'mimo') {
            model = settings.mimoModel || 'mimo/mimo-auto';
        }

        await invoke('ai_start', {
            config: {
                model,
                apiKey,
                aiProvider: settings.aiModel,
                ollamaEndpoint: settings.ollamaEndpoint || null,
                enableToolUse: true,
                projectPath: project?.path ?? null,
                idfPath: idfStatus.active?.idf_path || settings.idfPath || null,
            },
        });
    }
}

export async function stopAI(): Promise<void> {
    if (isTauri()) {
        try {
            const { invoke } = await import('@tauri-apps/api/core');
            await invoke('ai_stop');
        } catch { /* 忽略 */ }
    }
}
