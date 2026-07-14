import { ControlPanel } from "./components/ControlPanel";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { StatusSidebar } from "./components/StatusSidebar";
import { Workspace } from "./components/Workspace";
import { useBridgeState } from "./useBridgeState";

function BridgeApplication() {
  const bridge = useBridgeState();

  return (
    <div className="bridge-shell" aria-busy={bridge.loading.initial}>
      <nav className="skip-links" aria-label="Skip links">
        <a href="#work-surfaces">Skip to work surfaces</a>
        <a href="#control-panel">Skip to control panel</a>
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

      <StatusSidebar
        proxyStatus={bridge.proxyStatus}
        browserStatus={bridge.browserStatus}
        a2aStatus={bridge.a2aStatus}
        trace={bridge.trace}
        chat={bridge.chat}
        sending={bridge.sending}
        modelSelected={Boolean(bridge.selectedModel)}
        onSend={bridge.sendPrompt}
        onRetry={bridge.retryLastPrompt}
        onClearTrace={bridge.clearTrace}
        onClearChat={bridge.clearChat}
      />
      <Workspace
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
      />
      <ControlPanel
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
          bridge.loading.proxy || bridge.loading.models || bridge.loading.login
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
      />
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
