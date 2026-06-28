import React from 'react';
import i18n from '../../i18n';

interface Props {
  children: React.ReactNode;
  /** Optional custom fallback node. If not provided, a default UI is shown. */
  fallback?: React.ReactNode;
  /** Component name for error context (shown in console). */
  name?: string;
  /** When true, renders the fallback inline (no fixed overlay). Useful for panels. */
  inline?: boolean;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    const label = this.props.name ? `[ErrorBoundary:${this.props.name}]` : '[ErrorBoundary]';
    console.error(label, error, errorInfo);
  }

  handleReload = () => {
    window.location.reload();
  };

  handleReset = () => {
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback;
      }

      // Inline mode: render inside a panel without fullscreen overlay
      if (this.props.inline) {
        return (
          <div className="h-full flex flex-col items-center justify-center p-6 text-center">
            <div className="w-10 h-10 mx-auto mb-3 rounded-full bg-error/10 flex items-center justify-center">
              <span className="text-xl">⚠</span>
            </div>
            <h3 className="text-[14px] font-semibold text-text-primary mb-1">
              {i18n.t('common.errorBoundary.title')}
            </h3>
            <p className="text-[12px] text-text-tertiary mb-3 max-w-sm">
              {this.state.error?.message || i18n.t('common.errorBoundary.unknownError')}
            </p>
            <button
              onClick={this.handleReset}
              className="px-3 py-1.5 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors"
            >
              {i18n.t('common.errorBoundary.retry')}
            </button>
          </div>
        );
      }

      // Default: fullscreen overlay
      return (
        <div className="fixed inset-0 bg-surface-base flex items-center justify-center z-[9999]">
          <div className="max-w-md w-full mx-4 p-6 bg-surface-elevated border border-border-default rounded-xl shadow-xl text-center">
            <div className="w-12 h-12 mx-auto mb-4 rounded-full bg-error/10 flex items-center justify-center">
              <span className="text-2xl">⚠</span>
            </div>
            <h2 className="text-[16px] font-semibold text-text-primary mb-2">
              {i18n.t('common.errorBoundary.title')}
            </h2>
            <p className="text-[13px] text-text-tertiary mb-1">
              {this.state.error?.message || i18n.t('common.errorBoundary.unknownError')}
            </p>
            <pre className="text-[11px] text-text-disabled bg-surface-overlay rounded-lg p-3 mb-4 text-left overflow-auto max-h-32 font-mono">
              {this.state.error?.stack?.split('\n').slice(0, 5).join('\n')}
            </pre>
            <div className="flex gap-2 justify-center">
              <button
                onClick={this.handleReset}
                className="px-4 py-2 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors"
              >
                {i18n.t('common.errorBoundary.retry')}
              </button>
              <button
                onClick={this.handleReload}
                className="px-4 py-2 text-[12px] font-medium bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors"
              >
                {i18n.t('common.errorBoundary.reload')}
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

export default ErrorBoundary;
