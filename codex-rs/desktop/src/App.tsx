import { RefreshCw, X } from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";

import { backend } from "./backend";
import { AppNavigation, type ToolView } from "./components/AppNavigation";
import { ControlPanel } from "./components/ControlPanel";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { LegacyDataMigrationDialog } from "./components/LegacyDataMigrationDialog";
import { StatusSidebar } from "./components/StatusSidebar";
import { Workspace } from "./components/Workspace";
import {
  archiveFilename,
  clearAllFrontendData,
  clearLegacyData,
  createLegacyDataArchive,
  detectLegacyData,
  downloadJsonArchive,
  loadLocalPreferences,
  saveLocalPreferences,
  type ThemePreference,
} from "./localData";
import {
  createLocalDataState,
  localDataReducer,
} from "./localDataReducer";
import { useAgentRuntime } from "./useAgentRuntime";
import { useBridgeState } from "./useBridgeState";
import { useMediaQuery } from "./useMediaQuery";

function BridgeApplication() {
  const bridge = useBridgeState();
  const agent = useAgentRuntime(bridge.selectedModel);
  const [initialPreferences] = useState(loadLocalPreferences);
  const [activeTool, setActiveTool] = useState<ToolView | null>(null);
  const [navigationCollapsed, setNavigationCollapsed] = useState(
    initialPreferences.navigationCollapsed,
  );
  const [localDataState, dispatchLocalData] = useReducer(
    localDataReducer,
    undefined,
    () => createLocalDataState(detectLegacyData()),
  );
  const drawerTitleRef = useRef<HTMLHeadingElement>(null);
  const drawerInvokerRef = useRef<HTMLButtonElement | null>(null);
  const pendingDrawerFocus = useRef(false);
  const skipNextPreferenceWrite = useRef(false);
  const [preferencesGeneration, setPreferencesGeneration] = useState(0);
  const [theme, setTheme] = useState(initialPreferences.theme);
  const systemUsesDarkTheme = useMediaQuery("(prefers-color-scheme: dark)");
  const compactLayout = useMediaQuery("(max-width: 1023px)");
  const [repositoryRoot, setRepositoryRoot] = useState("");
  const repositoryRootWasEdited = useRef(false);
  const trace = useMemo(
    () => [...bridge.trace, ...agent.activity].slice(-240),
    [agent.activity, bridge.trace],
  );

  useEffect(() => {
    const codeMemoryRoot = bridge.codeMemoryStatus?.root?.trim();
    if (
      !repositoryRootWasEdited.current &&
      !repositoryRoot &&
      codeMemoryRoot
    ) {
      repositoryRootWasEdited.current = true;
      setRepositoryRoot(codeMemoryRoot);
    }
  }, [bridge.codeMemoryStatus?.root, repositoryRoot]);

  useEffect(() => {
    document.documentElement.dataset.theme =
      theme === "system" ? (systemUsesDarkTheme ? "dark" : "light") : theme;
    if (skipNextPreferenceWrite.current) {
      skipNextPreferenceWrite.current = false;
      return;
    }
    saveLocalPreferences({ version: 2, theme, navigationCollapsed });
  }, [navigationCollapsed, preferencesGeneration, systemUsesDarkTheme, theme]);

  useEffect(() => {
    if (!activeTool || !pendingDrawerFocus.current) {
      return;
    }
    pendingDrawerFocus.current = false;
    const frame = window.requestAnimationFrame(() => drawerTitleRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [activeTool]);

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

  const closeToolDrawer = useCallback(() => {
    setActiveTool(null);
    window.requestAnimationFrame(() => {
      if (drawerInvokerRef.current?.isConnected) {
        drawerInvokerRef.current.focus();
      }
    });
  }, []);

  const selectTool = useCallback(
    (tool: ToolView, trigger: HTMLButtonElement) => {
      drawerInvokerRef.current = trigger;
      setActiveTool((current) => {
        if (current === tool) {
          window.requestAnimationFrame(() => trigger.focus());
          return null;
        }
        pendingDrawerFocus.current = true;
        return tool;
      });
    },
    [],
  );

  const openRouting = useCallback((trigger: HTMLButtonElement) => {
    drawerInvokerRef.current = trigger;
    pendingDrawerFocus.current = true;
    setActiveTool("routing");
  }, []);

  const toggleNavigation = useCallback(() => {
    setNavigationCollapsed((collapsed) => !collapsed);
  }, []);

  const changeTheme = useCallback((nextTheme: ThemePreference) => {
    setTheme(nextTheme);
  }, []);

  const startNewConversation = useCallback(() => {
    void agent.startThread(repositoryRoot);
    setActiveTool(null);
  }, [agent.startThread, repositoryRoot]);

  const changeRepositoryRoot = useCallback((root: string) => {
    repositoryRootWasEdited.current = true;
    setRepositoryRoot(root);
  }, []);

  const exportLegacyData = useCallback(() => {
    try {
      downloadJsonArchive(
        createLegacyDataArchive(),
        archiveFilename("legacy-conversations"),
      );
      dispatchLocalData({ type: "legacy_exported" });
    } catch (error) {
      dispatchLocalData({
        type: "operation_failed",
        error:
          error instanceof Error
            ? error.message
            : "Bridge could not export the saved conversations.",
      });
    }
  }, []);

  const removeLegacyData = useCallback(() => {
    try {
      clearLegacyData();
      dispatchLocalData({ type: "legacy_cleared" });
    } catch (error) {
      dispatchLocalData({
        type: "operation_failed",
        error:
          error instanceof Error
            ? error.message
            : "Bridge could not remove the saved browser copy.",
      });
    }
  }, []);

  const exportSession = useCallback(() => {
    void agent
      .exportSessionArchive()
      .then((archive) =>
        downloadJsonArchive(archive, archiveFilename("session")),
      );
  }, [agent.exportSessionArchive]);

  const archiveCurrentConversation = useCallback(() => {
    const archive = agent.exportCurrentThreadArchive();
    if (archive) {
      downloadJsonArchive(archive, archiveFilename("conversation"));
      void agent.archiveThread();
    }
  }, [agent.archiveThread, agent.exportCurrentThreadArchive]);

  const deleteAllLocalData = useCallback(async () => {
    const nativeCleanup = await backend.deleteAllLocalDataIfAvailable();
    skipNextPreferenceWrite.current = true;
    clearAllFrontendData();
    bridge.resetLocalSession();
    setActiveTool(null);
    setNavigationCollapsed(false);
    setTheme("system");
    setPreferencesGeneration((generation) => generation + 1);
    return nativeCleanup;
  }, [bridge.resetLocalSession]);

  return (
    <div
      className="bridge-shell"
      data-navigation-collapsed={
        navigationCollapsed || compactLayout || undefined
      }
      data-compact-layout={compactLayout || undefined}
      data-tool-open={Boolean(activeTool) || undefined}
      aria-busy={bridge.loading.initial || agent.loading}
    >
      <nav className="skip-links" aria-label="Skip links">
        <a href="#agent-activity">Skip to current task</a>
        {activeTool && <a href="#tool-drawer">Skip to workbench</a>}
      </nav>

      {(bridge.loading.initial || agent.loading) && (
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
      {agent.error && (
        <div className="application-error" role="alert">
          <p>{agent.error}</p>
          <button type="button" onClick={() => void agent.refreshThreads(true)}>
            Refresh app-server threads
          </button>
        </div>
      )}

      <LegacyDataMigrationDialog
        state={localDataState}
        onExport={exportLegacyData}
        onClear={removeLegacyData}
      />

      <AppNavigation
        activeTool={activeTool}
        collapsed={navigationCollapsed || compactLayout}
        compact={compactLayout}
        theme={theme}
        proxyStatus={bridge.proxyStatus}
        browserStatus={bridge.browserStatus}
        a2aStatus={bridge.a2aStatus}
        a2aTasks={bridge.a2aTasks}
        conversations={agent.threadSummaries}
        activeConversationId={agent.activeThreadId}
        repositoryRoot={repositoryRoot}
        selectedModel={bridge.selectedModel}
        sending={agent.sending}
        onToggleCollapsed={toggleNavigation}
        onThemeChange={changeTheme}
        onSelectTool={selectTool}
        onNewTask={startNewConversation}
        onSelectConversation={(id) => void agent.resumeThread(id)}
        onRepositoryRootChange={changeRepositoryRoot}
      />
      <StatusSidebar
        proxyStatus={bridge.proxyStatus}
        browserStatus={bridge.browserStatus}
        a2aStatus={bridge.a2aStatus}
        trace={trace}
        chat={agent.messages}
        activeThreadName={agent.activeThread?.name ?? ""}
        hasActiveThread={Boolean(agent.activeThread)}
        pendingRequests={agent.pendingRequests}
        sending={agent.sending}
        selectedModel={bridge.selectedModel}
        onSend={(prompt) => agent.sendPrompt(prompt, repositoryRoot)}
        onRetry={() => agent.retryLastPrompt(repositoryRoot)}
        onInterrupt={agent.interruptTurn}
        onForkThread={agent.forkThread}
        onArchiveThread={agent.archiveThread}
        onNameThread={agent.nameThread}
        onResolveRequest={agent.resolveRequest}
        onDenyRequest={agent.denyRequest}
        onExecuteDynamicTool={agent.executeDynamicTool}
        onClearTrace={() => {
          bridge.clearTrace();
          agent.clearActivity();
        }}
        onOpenRouting={openRouting}
      />
      {activeTool && (
        <aside
          id="tool-drawer"
          className="tool-drawer"
          aria-labelledby="tool-drawer-title"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              closeToolDrawer();
            }
          }}
        >
          <header className="tool-drawer__header">
            <div>
              <p className="eyebrow">Workbench</p>
              <h2 id="tool-drawer-title" ref={drawerTitleRef} tabIndex={-1}>
                {workbenchTitle}
              </h2>
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
                onClick={closeToolDrawer}
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
                runtimeStatus={agent.status}
                routingBusy={
                  bridge.loading.proxy ||
                  bridge.loading.models
                }
                a2aBusy={bridge.a2aBusy}
                codeMemoryBusy={bridge.codeMemoryBusy}
                routingError={bridge.errors.models}
                a2aError={bridge.errors.a2a}
                codeMemoryError={bridge.errors.codeMemory}
                onRefreshAll={bridge.refreshAll}
                onSelectModel={bridge.setSelectedModel}
                onRefreshModels={bridge.refreshModels}
                onConfigureProxy={bridge.configureProxy}
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
                onConfigureA2aServer={bridge.configureA2aServer}
                onProvisionA2aToken={bridge.provisionA2aToken}
                onDeleteA2aToken={bridge.deleteA2aToken}
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
                localDataState={localDataState}
                dispatchLocalData={dispatchLocalData}
                conversationCount={agent.threadSummaries.length}
                currentMessageCount={agent.messages.length}
                localDataBusy={agent.sending}
                onExportSession={exportSession}
                onArchiveCurrent={archiveCurrentConversation}
                onDeleteAllLocalData={deleteAllLocalData}
                onExportSupportBundle={backend.exportSupportBundle}
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
