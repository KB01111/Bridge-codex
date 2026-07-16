import { RefreshCw, X } from "lucide-react";
import { useEffect, useState } from "react";

import { AppNavigation, type ToolView } from "./components/AppNavigation";
import { ControlPanel } from "./components/ControlPanel";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { StatusSidebar } from "./components/StatusSidebar";
import { Workspace } from "./components/Workspace";
import { useBridgeState } from "./useBridgeState";

function BridgeApplication() {
  const bridge = useBridgeState();
  const [activeTool, setActiveTool] = useState<ToolView | null>(null);
  const [navigationCollapsed, setNavigationCollapsed] = useState(false);
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    try {
      const saved = window.localStorage.getItem("bridge-codex-theme");
      if (saved === "light" || saved === "dark") {
        return saved;
      }
    } catch {
      // Theme persistence is an enhancement; the system preference still works.
    }
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  });

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      window.localStorage.setItem("bridge-codex-theme", theme);
    } catch {
      // Keep the active theme even when local storage is unavailable.
    }
  }, [theme]);

  const workbenchTitle =
    activeTool === "browser"
      ? "Agent browser"
      : activeTool === "desktop"
        ? "Desktop control"
        : activeTool === "tasks"
          ? "Delegated tasks"
          : activeTool === "memory"
            ? "Code memory"
            : "Models & routing";

  function selectTool(tool: ToolView) {
    setActiveTool((current) => (current === tool ? null : tool));
  }

  function startNewConversation() {
    bridge.newConversation();
    setActiveTool(null);
  }

  return (
    <div
      className="bridge-shell"
      data-navigation-collapsed={navigationCollapsed || undefined}
      data-tool-open={Boolean(activeTool) || undefined}
      aria-busy={bridge.loading.initial}
    >
      <nav className="skip-links" aria-label="Skip links">
        <a href="#agent-activity">Skip to current task</a>
        {activeTool && <a href="#tool-drawer">Skip to workbench</a>}
      </nav>

      {bridge.loading.initial && (
        <p className="application-status" role="status">
          Connecting to local Bridge Codex services…
        </p>
      )}
      {bridge.errors.initialization && (
        <div className="application-error" role="alert">
          <p>{bridge.errors.initialization}</p>
          <button type="button" onClick={() => void bridge.refreshAll()}>
            Retry initialization
          </button>
        </div>
      )}

      <AppNavigation
        activeTool={activeTool}
        collapsed={navigationCollapsed}
        theme={theme}
        proxyStatus={bridge.proxyStatus}
        browserStatus={bridge.browserStatus}
        a2aStatus={bridge.a2aStatus}
        a2aTasks={bridge.a2aTasks}
        conversations={bridge.conversations}
        activeConversationId={bridge.activeConversationId}
        selectedModel={bridge.selectedModel}
        sending={bridge.sending}
        onToggleCollapsed={() =>
          setNavigationCollapsed((collapsed) => !collapsed)
        }
        onToggleTheme={() =>
          setTheme((current) => (current === "dark" ? "light" : "dark"))
        }
        onSelectTool={selectTool}
        onNewTask={startNewConversation}
        onSelectConversation={bridge.selectConversation}
      />
      <StatusSidebar
        proxyStatus={bridge.proxyStatus}
        browserStatus={bridge.browserStatus}
        a2aStatus={bridge.a2aStatus}
        trace={bridge.trace}
        chat={bridge.chat}
        sending={bridge.sending}
        selectedModel={bridge.selectedModel}
        onSend={bridge.sendPrompt}
        onRetry={bridge.retryLastPrompt}
        onClearTrace={bridge.clearTrace}
        onClearChat={bridge.clearChat}
        onOpenRouting={() => setActiveTool("routing")}
      />
      {activeTool && (
        <aside
          id="tool-drawer"
          className="tool-drawer"
          aria-label={`${workbenchTitle} workbench`}
        >
          <header className="tool-drawer__header">
            <div>
              <p className="eyebrow">Workbench</p>
              <h2>{workbenchTitle}</h2>
            </div>
            <div>
              <button
                className="icon-button"
                type="button"
                aria-label="Refresh local services"
                title="Refresh local services"
                onClick={() => void bridge.refreshAll()}
              >
                <RefreshCw aria-hidden="true" />
              </button>
              <button
                className="icon-button"
                type="button"
                aria-label="Close workbench"
                title="Close workbench"
                onClick={() => setActiveTool(null)}
              >
                <X aria-hidden="true" />
              </button>
            </div>
          </header>
          <div className="tool-drawer__content">
            {activeTool === "browser" || activeTool === "desktop" ? (
              <Workspace
                activeTab={activeTool}
                browserStatus={bridge.browserStatus}
                browserFrame={bridge.browserFrame}
                desktopStatus={bridge.desktopStatus}
                browserBusy={bridge.browserBusy}
                desktopBusy={bridge.desktopBusy}
                browserError={bridge.errors.browser}
                desktopError={bridge.errors.desktop}
                onStartBrowser={bridge.startBrowser}
                onStopBrowser={bridge.stopBrowser}
                onNavigate={bridge.navigateBrowser}
                onBrowserClickSelector={bridge.clickBrowserSelector}
                onBrowserClickAt={bridge.clickBrowserAt}
                onBrowserType={bridge.typeInBrowser}
                onRefreshDesktop={bridge.refreshDesktopStatus}
                onEnableDesktop={bridge.enableDesktopWorkMode}
                onDisableDesktop={bridge.disableDesktopWorkMode}
                onDesktopClickAt={bridge.clickDesktopAt}
                onDesktopType={bridge.typeOnDesktop}
                onActiveTabChange={setActiveTool}
              />
            ) : (
              <ControlPanel
                activeTab={activeTool}
                compact
                proxyStatus={bridge.proxyStatus}
                a2aStatus={bridge.a2aStatus}
                a2aTasks={bridge.a2aTasks}
                selectedA2aTask={bridge.selectedA2aTask}
                codePolicyStatus={bridge.codePolicyStatus}
                codeMemoryStatus={bridge.codeMemoryStatus}
                codeMemoryResults={bridge.codeMemoryResults}
                codeMemoryWarnings={bridge.codeMemoryWarnings}
                models={bridge.models}
                selectedModel={bridge.selectedModel}
                loginState={bridge.loginState}
                loginVerification={bridge.loginVerification}
                routingBusy={
                  bridge.loading.proxy ||
                  bridge.loading.models ||
                  bridge.loading.login
                }
                a2aBusy={bridge.a2aBusy}
                codeMemoryBusy={bridge.codeMemoryBusy}
                routingError={bridge.errors.models}
                a2aError={bridge.errors.a2a}
                codeMemoryError={bridge.errors.codeMemory}
                onRefreshAll={bridge.refreshAll}
                onSelectModel={bridge.setSelectedModel}
                onRefreshModels={bridge.refreshModels}
                onEnsureProxy={bridge.ensureProxy}
                onLaunchLogin={bridge.launchLogin}
                onVerifyLogin={async () => {
                  await bridge.verifyLogin();
                }}
                onResetLogin={bridge.resetLogin}
                onRefreshA2aTasks={bridge.refreshA2aTasks}
                onSelectA2aTask={(id) => {
                  void bridge.selectA2aTask(id);
                }}
                onDelegateA2aTask={async (request) => {
                  await bridge.delegateA2aTask(request);
                }}
                onCancelA2aTask={async (id) => {
                  await bridge.cancelA2aTask(id);
                }}
                onRefreshCodeMemory={bridge.refreshCodeMemoryStatus}
                onIndexCodeMemory={async (root) => {
                  await bridge.indexCodeMemory(root);
                }}
                onSearchCodeMemory={async (request) => {
                  await bridge.searchCodeMemory(request.query, {
                    maxResults: request.maxResults,
                    graphWeight: request.graphWeight,
                  });
                }}
                onClearCodeMemory={bridge.clearCodeMemory}
                onClearCodeMemoryResults={bridge.clearCodeMemoryResults}
                onActiveTabChange={setActiveTool}
              />
            )}
          </div>
        </aside>
      )}
    </div>
  );
}

export function App() {
  return (
    <ErrorBoundary>
      <BridgeApplication />
    </ErrorBoundary>
  );
}
