import "@fontsource-variable/inter";
import "@fontsource-variable/noto-sans-thai";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import "./i18n";

import { Component, type ReactNode, StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import { bootstrapTheme } from "./stores/settings";

bootstrapTheme();

/** Last line of defence: a rendering bug shows what went wrong instead of a blank window. */
class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-surface p-8 text-center">
        <div className="text-[15px] font-semibold">PuriCat hit an unexpected error</div>
        <pre data-selectable className="max-h-48 max-w-2xl overflow-auto rounded-lg bg-sunken p-3 text-left font-mono text-xs whitespace-pre-wrap text-fg-muted">
          {error.message}
        </pre>
        <button
          type="button"
          onClick={() => location.reload()}
          className="h-8 rounded-lg bg-accent px-3.5 text-[13px] font-semibold text-accent-fg hover:bg-accent-hover"
        >
          Reload
        </button>
      </div>
    );
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
