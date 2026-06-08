/**
 * CodeEditor - Monaco 代码编辑器组件 (Codex-inspired)
 *
 * 功能：
 * - 多标签页管理
 * - C/C++ 语法高亮
 * - ESP-IDF 代码片段
 * - 文件修改状态指示
 */

import { useRef, useCallback, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import Editor, { OnMount, Monaco } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';

// 配置 Monaco 使用本地打包而非 CDN（生产环境 CSP 阻止 CDN 加载）
import { loader } from '@monaco-editor/react';
import * as monaco from 'monaco-editor';
loader.config({ monaco });
import { X, FileCode, FileJson, FileText, FileCog, Image } from 'lucide-react';
import { useFileStore } from '../../stores';

const ESP32_THEME = {
  base: 'vs-dark' as const,
  inherit: true,
  rules: [
    { token: 'comment', foreground: '6A9955' },
    { token: 'keyword', foreground: '569CD6' },
    { token: 'string', foreground: 'CE9178' },
    { token: 'number', foreground: 'B5CEA8' },
    { token: 'type', foreground: '4EC9B0' },
    { token: 'function', foreground: 'DCDCAA' },
    { token: 'variable', foreground: '9CDCFE' },
    { token: 'macro', foreground: '4FC1FF' },
  ],
  colors: {
    'editor.background': '#0d0d0d',
    'editor.foreground': '#ececee',
    'editor.lineHighlightBackground': '#1a1a1e',
    'editor.selectionBackground': '#264f78',
    'editorCursor.foreground': '#AEAFAD',
    'editorLineNumber.foreground': '#52525b',
    'editorLineNumber.activeForeground': '#a1a1aa',
    'editor.inactiveSelectionBackground': '#3e3e4a',
    'editorWidget.background': '#1a1a1e',
    'editorWidget.border': '#2e2e38',
  },
};

function registerTheme(monaco: Monaco) {
  monaco.editor.defineTheme('esp32-dark', ESP32_THEME);
}

function registerCompletions(monaco: Monaco) {
  monaco.languages.registerCompletionItemProvider('c', {
    provideCompletionItems: (model: editor.ITextModel, position: Parameters<Parameters<typeof monaco.languages.registerCompletionItemProvider>[1]['provideCompletionItems']>[1]) => {
      const word = model.getWordUntilPosition(position);
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };

      const suggestions = [
        { label: 'ESP_LOGI', kind: monaco.languages.CompletionItemKind.Function, insertText: 'ESP_LOGI(TAG, "${1:format}", ${2:args});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'ESP-IDF info log macro', range },
        { label: 'ESP_LOGE', kind: monaco.languages.CompletionItemKind.Function, insertText: 'ESP_LOGE(TAG, "${1:format}", ${2:args});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'ESP-IDF error log macro', range },
        { label: 'ESP_LOGW', kind: monaco.languages.CompletionItemKind.Function, insertText: 'ESP_LOGW(TAG, "${1:format}", ${2:args});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'ESP-IDF warning log macro', range },
        { label: 'ESP_LOGD', kind: monaco.languages.CompletionItemKind.Function, insertText: 'ESP_LOGD(TAG, "${1:format}", ${2:args});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'ESP-IDF debug log macro', range },
        { label: 'gpio_set_direction', kind: monaco.languages.CompletionItemKind.Function, insertText: 'gpio_set_direction(${1:GPIO_NUM_XX}, ${2:GPIO_MODE_INPUT_OUTPUT});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Set GPIO direction', range },
        { label: 'gpio_set_level', kind: monaco.languages.CompletionItemKind.Function, insertText: 'gpio_set_level(${1:GPIO_NUM_XX}, ${2:level});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Set GPIO output level', range },
        { label: 'gpio_get_level', kind: monaco.languages.CompletionItemKind.Function, insertText: 'gpio_get_level(${1:GPIO_NUM_XX});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Get GPIO input level', range },
        { label: 'i2c_driver_install', kind: monaco.languages.CompletionItemKind.Function, insertText: 'i2c_driver_install(${1:I2C_NUM_0}, ${2:I2C_MODE_MASTER}, ${3:0}, ${4:0}, ${5:0});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Install I2C driver', range },
        { label: 'i2c_param_config', kind: monaco.languages.CompletionItemKind.Function, insertText: 'i2c_param_config(${1:I2C_NUM_0}, &${2:conf});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Configure I2C parameters', range },
        { label: 'i2c_master_write_byte', kind: monaco.languages.CompletionItemKind.Function, insertText: 'i2c_master_write_byte(${1:i2c_cmd_handle_t cmd}, ${2:device_addr << 1} | ${3:I2C_MASTER_WRITE}, ${4:true});', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'I2C master write byte', range },
        { label: 'vTaskDelay', kind: monaco.languages.CompletionItemKind.Function, insertText: 'vTaskDelay(pdMS_TO_TICKS(${1:1000}));', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Delay task', range },
        { label: 'xTaskCreate', kind: monaco.languages.CompletionItemKind.Function, insertText: 'xTaskCreate(\n    ${1:taskFunction},   // Task function\n    "${2:taskName}",      // Task name\n    ${3:4096},            // Stack size\n    ${4:NULL},            // Parameters\n    ${5:1},               // Priority\n    ${6:NULL}             // Task handle\n);', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Create a FreeRTOS task', range },
        { label: 'wifi_init', kind: monaco.languages.CompletionItemKind.Snippet, insertText: 'wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();\nESP_ERROR_CHECK(esp_wifi_init(&cfg));\nESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));\nESP_ERROR_CHECK(esp_wifi_start());', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'Initialize WiFi', range },
        { label: 'app_main', kind: monaco.languages.CompletionItemKind.Snippet, insertText: 'void app_main(void)\n{\n    ${1:// Initialization code}\n    printf("${2:Hello from ESP32!}\\n");\n}', insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet, documentation: 'ESP-IDF app_main entry point', range },
      ];

      return { suggestions };
    },
  });
}

const FILE_ICONS: Record<string, React.ComponentType<{ size?: number | string; className?: string }>> = {
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
  svg: Image,
};

function getTabIcon(filename: string) {
  const ext = filename.split('.').pop()?.toLowerCase() || '';
  return FILE_ICONS[ext] || FileText;
}

function getLanguage(filename: string): string {
  const ext = filename.split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'c':
    case 'h': return 'c';
    case 'cpp':
    case 'hpp': return 'cpp';
    case 'py': return 'python';
    case 'json': return 'json';
    case 'md': return 'markdown';
    case 'yaml':
    case 'yml': return 'yaml';
    default: return 'plaintext';
  }
}

interface TabBarProps {
  tabs: { id: string; name: string; modified?: boolean; deleted?: boolean }[];
  activeTabId: string | null;
  onTabClick: (id: string) => void;
  onTabClose: (id: string) => void;
}

function TabBar({ tabs, activeTabId, onTabClick, onTabClose }: TabBarProps) {
  return (
    <div className="flex h-9 bg-surface-elevated border-b border-border-default overflow-x-auto scrollbar-hidden">
      {tabs.map((tab) => {
        const Icon = getTabIcon(tab.name);
        const isActive = tab.id === activeTabId;
        return (
          <div
            key={tab.id}
            className={`
              group flex items-center gap-1.5 pl-3 pr-2 h-full cursor-pointer border-r border-border-subtle shrink-0
              transition-all duration-150 select-none
              ${isActive
                ? 'bg-surface-root text-text-primary border-t-2 border-t-accent'
                : tab.deleted
                  ? 'bg-surface-elevated text-error border-t-2 border-t-transparent'
                  : 'bg-surface-elevated text-text-tertiary hover:bg-surface-hover hover:text-text-secondary border-t-2 border-t-transparent'
              }
            `}
            onClick={() => onTabClick(tab.id)}
          >
            <Icon size={13} className={`shrink-0 ${tab.deleted ? 'text-error' : ''}`} />
            <span className={`text-[12px] truncate max-w-[140px] ${tab.deleted ? 'line-through' : ''}`}>
              {tab.name}
            </span>
            {tab.modified && (
              <div className="w-2 h-2 rounded-full bg-warning shrink-0 ml-0.5" />
            )}
            <button
              className="ml-1 p-0.5 rounded-sm opacity-0 group-hover:opacity-100 hover:bg-surface-active text-text-tertiary hover:text-text-primary transition-all"
              onClick={(e) => {
                e.stopPropagation();
                onTabClose(tab.id);
              }}
            >
              <X size={12} />
            </button>
          </div>
        );
      })}
    </div>
  );
}

export function CodeEditor() {
  const { t } = useTranslation();
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);
  const { tabs, activeTabId, setActiveTab, closeTab, updateTabContent, saveFile, updateCursorPosition, updateEditorLanguage } = useFileStore();

  const handleEditorMount: OnMount = (editor, monaco) => {
    editorRef.current = editor;
    registerTheme(monaco);
    registerCompletions(monaco);
    monaco.editor.setTheme('esp32-dark');

    editor.onDidChangeCursorPosition((e) => {
      updateCursorPosition(e.position.lineNumber, e.position.column);
    });
  };

  const handleEditorChange = useCallback((value: string | undefined) => {
    if (activeTabId && value !== undefined) {
      updateTabContent(activeTabId, value);
    }
  }, [activeTabId, updateTabContent]);

  const activeTab = tabs.find((t) => t.id === activeTabId);

  useEffect(() => {
    if (activeTab) {
      updateEditorLanguage(getLanguage(activeTab.name));
      if (editorRef.current) {
        const pos = editorRef.current.getPosition();
        if (pos) {
          updateCursorPosition(pos.lineNumber, pos.column);
        }
      }
    } else {
      updateEditorLanguage('');
      updateCursorPosition(1, 1);
    }
  }, [activeTab?.id, updateEditorLanguage, updateCursorPosition]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        if (activeTabId) saveFile(activeTabId);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [activeTabId, saveFile]);

  return (
    <div className="h-full flex flex-col">
      {/* Tab Bar */}
      <TabBar
        tabs={tabs}
        activeTabId={activeTabId}
        onTabClick={setActiveTab}
        onTabClose={closeTab}
      />

      {/* Editor */}
      <div className="flex-1 min-h-0">
        {activeTab ? (
          <Editor
            height="100%"
            language={getLanguage(activeTab.name)}
            value={activeTab.content}
            onChange={handleEditorChange}
            onMount={handleEditorMount}
            theme="esp32-dark"
            options={{
              fontSize: 13,
              fontFamily: "'JetBrains Mono', 'Cascadia Code', 'Fira Code', Consolas, monospace",
              minimap: { enabled: true, scale: 1, showSlider: 'mouseover' },
              lineNumbers: 'on',
              renderWhitespace: 'selection',
              bracketPairColorization: { enabled: true },
              automaticLayout: true,
              scrollBeyondLastLine: false,
              wordWrap: 'off',
              tabSize: 4,
              insertSpaces: true,
              smoothScrolling: true,
              cursorBlinking: 'smooth',
              cursorSmoothCaretAnimation: 'on',
              padding: { top: 8 },
              glyphMargin: false,
              folding: true,
              lineDecorationsWidth: 8,
            }}
            loading={
              <div className="h-full flex items-center justify-center bg-surface-root">
                <div className="text-text-tertiary text-[13px]">Loading editor...</div>
              </div>
            }
          />
        ) : (
          <div className="h-full flex items-center justify-center bg-surface-root">
            <div className="text-center animate-fade-in">
              <div className="w-16 h-16 rounded-2xl bg-surface-elevated border border-border-default flex items-center justify-center mx-auto mb-4">
                <FileCode size={28} className="text-text-disabled" />
              </div>
              <p className="text-[14px] text-text-secondary font-medium mb-1">{t('editor.selectFile')}</p>
              <p className="text-[12px] text-text-tertiary">{t('editor.createProject')}</p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default CodeEditor;