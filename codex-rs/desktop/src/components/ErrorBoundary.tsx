import { Component, type ErrorInfo, type ReactNode } from "react";

type ErrorBoundaryProps = {
  children: ReactNode;
};

type ErrorBoundaryState = {
  error: Error | null;
};

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Bridge Codex frontend crashed", error, info.componentStack);
  }

  render() {
    if (!this.state.error) {
      return this.props.children;
    }

    return (
      <main
        className="fatal-error"
        role="alert"
        aria-labelledby="fatal-error-title"
      >
        <p className="eyebrow">Interface error</p>
        <h1 id="fatal-error-title">Bridge Codex could not render</h1>
        <p>
          {this.state.error.message || "An unexpected frontend error occurred."}
        </p>
        <button type="button" onClick={() => window.location.reload()}>
          Reload application
        </button>
      </main>
    );
  }
}
