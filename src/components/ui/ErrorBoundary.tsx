import React from 'react';

interface Props {
  children: React.ReactNode;
  fallback?: React.ReactNode;
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
    console.error('[ErrorBoundary]', error, errorInfo);
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

      return (
        <div className="fixed inset-0 bg-surface-base flex items-center justify-center z-[9999]">
          <div className="max-w-md w-full mx-4 p-6 bg-surface-elevated border border-border-default rounded-xl shadow-xl text-center">
            <div className="w-12 h-12 mx-auto mb-4 rounded-full bg-error/10 flex items-center justify-center">
              <span className="text-2xl">⚠</span>
            </div>
            <h2 className="text-[16px] font-semibold text-text-primary mb-2">
              应用遇到了错误
            </h2>
            <p className="text-[13px] text-text-tertiary mb-1">
              {this.state.error?.message || '未知错误'}
            </p>
            <pre className="text-[11px] text-text-disabled bg-surface-overlay rounded-lg p-3 mb-4 text-left overflow-auto max-h-32 font-mono">
              {this.state.error?.stack?.split('\n').slice(0, 5).join('\n')}
            </pre>
            <div className="flex gap-2 justify-center">
              <button
                onClick={this.handleReset}
                className="px-4 py-2 text-[12px] font-medium bg-surface-overlay border border-border-subtle rounded-lg text-text-secondary hover:text-text-primary hover:bg-surface-hover transition-colors"
              >
                重试
              </button>
              <button
                onClick={this.handleReload}
                className="px-4 py-2 text-[12px] font-medium bg-accent text-white rounded-lg hover:bg-accent-hover transition-colors"
              >
                重新加载应用
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
