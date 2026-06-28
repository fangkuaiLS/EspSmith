import { useTranslation } from 'react-i18next';
import { useChatStore } from '../../stores/chatStore';
import { translateBackendString } from '../../i18n';

export function PermissionModal() {
  const { t } = useTranslation();
  const pendingPermission = useChatStore((s) => s.pendingPermission);
  const respondPermission = useChatStore((s) => s.respondPermission);

  if (!pendingPermission) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="bg-[#1e1e2e] border border-yellow-600/50 rounded-lg shadow-2xl w-[420px] max-w-[90vw] p-6">
        <div className="flex items-start gap-3 mb-4">
          <span className="text-2xl">⚠️</span>
          <div className="flex-1 min-w-0">
            <h3 className="text-yellow-400 font-semibold text-sm mb-1">{t('chat.permission.title')}</h3>
            <p className="text-gray-300 text-xs break-all">
              {pendingPermission.toolName}: {translateBackendString(pendingPermission.reason)}
            </p>
          </div>
        </div>
        <div className="flex gap-3 justify-end">
          <button
            onClick={() => respondPermission(false)}
            className="px-4 py-2 text-sm rounded bg-red-600/20 text-red-400 border border-red-600/30 hover:bg-red-600/30 transition-colors"
          >
            {t('chat.permission.deny')}
          </button>
          <button
            onClick={() => respondPermission(true)}
            className="px-4 py-2 text-sm rounded bg-green-600/20 text-green-400 border border-green-600/30 hover:bg-green-600/30 transition-colors"
          >
            {t('chat.permission.allow')}
          </button>
        </div>
      </div>
    </div>
  );
}