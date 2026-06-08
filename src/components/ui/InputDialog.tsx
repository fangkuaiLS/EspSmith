/**
 * InputDialog - 应用内输入对话框（替代浏览器 prompt）
 */

import { useState, useRef, useEffect } from 'react';
import { X } from 'lucide-react';

interface InputDialogProps {
  open: boolean;
  title: string;
  placeholder?: string;
  defaultValue?: string;
  label?: string;
  cancelLabel?: string;
  okLabel?: string;
  onConfirm: (value: string) => void;
  onCancel: () => void;
}

export function InputDialog({
  open,
  title,
  placeholder = '',
  defaultValue = '',
  label,
  cancelLabel = 'Cancel',
  okLabel = 'OK',
  onConfirm,
  onCancel,
}: InputDialogProps) {
  const [value, setValue] = useState(defaultValue);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setValue(defaultValue);
      // 延迟聚焦确保动画完成
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [open, defaultValue]);

  useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        if (value.trim()) onConfirm(value.trim());
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        onCancel();
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [open, value, onConfirm, onCancel]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 animate-fade-in">
      <div
        className="bg-surface-elevated border border-border-default rounded-xl w-[400px] shadow-2xl animate-scale-in"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="px-5 py-3.5 border-b border-border-default flex items-center justify-between">
          <h3 className="text-[14px] font-semibold text-text-primary">{title}</h3>
          <button
            onClick={onCancel}
            className="p-1 rounded-md text-text-tertiary hover:text-text-primary hover:bg-surface-hover transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        {/* Body */}
        <div className="px-5 py-4">
          {label && (
            <label className="block text-[11px] font-medium text-text-tertiary mb-2 uppercase tracking-wider">
              {label}
            </label>
          )}
          <input
            ref={inputRef}
            type="text"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder={placeholder}
            className="w-full px-3 py-2.5 text-[13px] bg-surface-overlay border border-border-subtle rounded-lg text-text-primary placeholder:text-text-disabled focus:outline-none focus:border-accent/50 transition-colors"
          />
        </div>

        {/* Footer */}
        <div className="px-5 py-3.5 border-t border-border-default flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="px-4 py-2 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-all"
          >
            {cancelLabel}
          </button>
          <button
            onClick={() => value.trim() && onConfirm(value.trim())}
            disabled={!value.trim()}
            className="px-4 py-2 text-[12px] font-medium bg-accent text-white rounded-lg hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-all"
          >
            {okLabel}
          </button>
        </div>
      </div>
    </div>
  );
}