/**
 * Toast - 应用内通知组件（替代浏览器 alert）
 */

import { useEffect, useState } from 'react';
import { X, AlertCircle, CheckCircle, Info, TriangleAlert } from 'lucide-react';
import { devLog } from '../../lib/devLog';

export interface ToastMessage {
  id: number;
  type: 'info' | 'success' | 'error' | 'warning';
  message: string;
}

let toastId = 0;
let addToastFn: ((msg: Omit<ToastMessage, 'id'>) => void) | null = null;

/** 全局 toast 调用（替代 alert） */
export function showToast(type: ToastMessage['type'], message: string) {
  if (addToastFn) {
    addToastFn({ type, message });
  } else {
    // 回退到 console（Toast 组件未挂载时）
    devLog(`[Toast:${type}] ${message}`);
  }
}

export function ToastContainer() {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  useEffect(() => {
    addToastFn = (msg) => {
      const id = ++toastId;
      setToasts((prev) => [...prev.slice(-4), { ...msg, id }]);
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id));
      }, 3500);
    };
    return () => { addToastFn = null; };
  }, []);

  if (toasts.length === 0) return null;

  const iconMap = {
    info: Info,
    success: CheckCircle,
    error: AlertCircle,
    warning: TriangleAlert,
  };

  const colorMap = {
    info: 'border-accent bg-accent-muted text-accent',
    success: 'border-success bg-success/10 text-success',
    error: 'border-error bg-error-muted text-error',
    warning: 'border-warning bg-warning/10 text-warning',
  };

  return (
    <div className="fixed bottom-6 right-6 z-[100] flex flex-col gap-2">
      {toasts.map((toast) => {
        const Icon = iconMap[toast.type];
        return (
          <div
            key={toast.id}
            className={`flex items-center gap-2.5 px-4 py-3 border rounded-lg shadow-xl min-w-[280px] max-w-[400px] animate-scale-in ${colorMap[toast.type]}`}
          >
            <Icon size={16} className="shrink-0" />
            <span className="text-[13px] flex-1">{toast.message}</span>
            <button
              onClick={() => setToasts((prev) => prev.filter((t) => t.id !== toast.id))}
              className="p-0.5 rounded-sm opacity-60 hover:opacity-100 transition-opacity shrink-0"
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}