import * as Dialog from "@radix-ui/react-dialog";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { useState } from "react";

import type { CodePolicyStatus, ProxyModel, ProxyStatus } from "../types";
import type { LoginState, LoginVerificationState } from "../useBridgeState";

export type RoutingPanelProps = {
  proxyStatus: ProxyStatus | null;
  codePolicyStatus: CodePolicyStatus | null;
  models: ProxyModel[];
  selectedModel: string;
  loginState: LoginState;
  loginVerification: LoginVerificationState;
  busy: boolean;
  error?: string | null;
  onSelectModel: (model: string) => void;
  onRefreshModels: () => Promise<void>;
  onEnsureProxy: () => Promise<void>;
  onLaunchLogin: () => Promise<void>;
  onVerifyLogin: () => Promise<void>;
  onResetLogin: () => void;
};

export function RoutingPanel({
  proxyStatus,
  codePolicyStatus,
  models,
  selectedModel,
  loginState,
  loginVerification,
  busy,
  error,
  onSelectModel,
  onRefreshModels,
  onEnsureProxy,
  onLaunchLogin,
  onVerifyLogin,
  onResetLogin,
}: RoutingPanelProps) {
  const [loginOpen, setLoginOpen] = useState(false);
  const selected = models.find((model) => model.id === selectedModel);

  function changeLoginOpen(open: boolean) {
    setLoginOpen(open);
    if (!open) {
      onResetLogin();
    }
  }

  return (
    <div className="routing-panel" aria-busy={busy}>
      <section className="model-control" aria-labelledby="model-heading">
        <div className="section-heading-row">
          <div>
            <h3 id="model-heading">Active model</h3>
            <p>Routes chat and delegated A2A tasks through CLIProxyAPI.</p>
          </div>
          <button
            type="button"
            disabled={busy}
            onClick={() => void onRefreshModels()}
          >
            {busy ? "Refreshing…" : "Refresh models"}
          </button>
        </div>

        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button
              className="model-trigger"
              type="button"
              aria-label="Choose active model"
              disabled={busy || models.length === 0}
            >
              <span>{selected?.id ?? "Select a model"}</span>
              <span aria-hidden="true">⌄</span>
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content
              className="model-menu"
              sideOffset={8}
              align="end"
            >
              <DropdownMenu.Label>Authenticated models</DropdownMenu.Label>
              <DropdownMenu.Separator />
              {models.length === 0 ? (
                <DropdownMenu.Item disabled>
                  No models reported
                </DropdownMenu.Item>
              ) : (
                <DropdownMenu.RadioGroup
                  value={selectedModel}
                  onValueChange={onSelectModel}
                >
                  {models.map((model) => (
                    <DropdownMenu.RadioItem key={model.id} value={model.id}>
                      <DropdownMenu.ItemIndicator aria-hidden="true">
                        ●
                      </DropdownMenu.ItemIndicator>
                      <span>{model.id}</span>
                      {model.ownedBy && <small>{model.ownedBy}</small>}
                    </DropdownMenu.RadioItem>
                  ))}
                </DropdownMenu.RadioGroup>
              )}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>

        {models.length === 0 && !busy && (
          <p className="empty-state">
            No authenticated models are available. Start the router and connect
            an account.
          </p>
        )}
        {selected && (
          <details className="model-details">
            <summary>Model details</summary>
            <dl>
              <div>
                <dt>Identifier</dt>
                <dd>{selected.id}</dd>
              </div>
              <div>
                <dt>Owner</dt>
                <dd>{selected.ownedBy ?? "Not reported"}</dd>
              </div>
              <div>
                <dt>Object</dt>
                <dd>{selected.object ?? "model"}</dd>
              </div>
            </dl>
          </details>
        )}
      </section>

      <section
        className="connection-control"
        aria-labelledby="connection-heading"
      >
        <h3 id="connection-heading">Account connection</h3>
        <p>
          {proxyStatus === null
            ? "Checking the local router…"
            : proxyStatus.running
              ? "CLIProxyAPI is ready for authenticated requests."
              : "Start the local router before connecting an account."}
        </p>
        {proxyStatus?.binaryPath && (
          <p title={proxyStatus.binaryPath}>
            Router binary: {proxyStatus.binaryPath}
          </p>
        )}
        {proxyStatus?.error && <p role="alert">{proxyStatus.error}</p>}
        {!proxyStatus?.running && (
          <button
            type="button"
            disabled={busy}
            onClick={() => void onEnsureProxy()}
          >
            {busy ? "Starting…" : "Start local router"}
          </button>
        )}

        <Dialog.Root open={loginOpen} onOpenChange={changeLoginOpen}>
          <Dialog.Trigger asChild>
            <button type="button" disabled={!proxyStatus?.running || busy}>
              Connect ChatGPT account
            </button>
          </Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay className="dialog-overlay" />
            <Dialog.Content className="login-dialog">
              <Dialog.Title>Connect a ChatGPT account</Dialog.Title>
              <Dialog.Description>
                CLIProxyAPI opens its Codex OAuth flow in the system browser.
                Credentials stay with the local router and are not handled by
                the React interface.
              </Dialog.Description>

              {loginState.phase === "idle" && (
                <div className="dialog-actions">
                  <Dialog.Close asChild>
                    <button type="button">Cancel</button>
                  </Dialog.Close>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void onLaunchLogin()}
                  >
                    Open browser login
                  </button>
                </div>
              )}
              {loginState.phase === "launching" && (
                <p role="status">Opening login…</p>
              )}
              {loginState.phase === "launched" && (
                <div>
                  {loginVerification.phase === "verified" ? (
                    <p role="status">
                      Connection verified with {loginVerification.modelCount}{" "}
                      model
                      {loginVerification.modelCount === 1 ? "" : "s"}.
                    </p>
                  ) : loginVerification.phase === "checking" ? (
                    <p role="status">
                      Waiting for OAuth completion (check{" "}
                      {loginVerification.attempt} of 60)…
                    </p>
                  ) : loginVerification.phase === "pending" ? (
                    <p role="alert">{loginVerification.error}</p>
                  ) : (
                    <p role="status">
                      Login opened in process {loginState.launch.processId}.
                      Complete it in the system browser, then verify the
                      connection.
                    </p>
                  )}
                  <div className="dialog-actions">
                    <Dialog.Close asChild>
                      <button type="button">
                        {loginVerification.phase === "verified"
                          ? "Done"
                          : "Verify later"}
                      </button>
                    </Dialog.Close>
                    {loginVerification.phase !== "verified" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void onVerifyLogin()}
                      >
                        {loginVerification.phase === "checking"
                          ? "Verifying…"
                          : "Verify connection"}
                      </button>
                    )}
                  </div>
                </div>
              )}
              {loginState.phase === "error" && (
                <div role="alert">
                  <p>{loginState.error}</p>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void onLaunchLogin()}
                  >
                    Try again
                  </button>
                </div>
              )}
              <Dialog.Close asChild>
                <button
                  className="dialog-close"
                  type="button"
                  aria-label="Close login dialog"
                >
                  ×
                </button>
              </Dialog.Close>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      </section>

      {error && <p role="alert">{error}</p>}

      <section className="sandbox-control" aria-labelledby="sandbox-heading">
        <h3 id="sandbox-heading">Execution contract</h3>
        {codePolicyStatus ? (
          <dl>
            <div>
              <dt>Parser</dt>
              <dd>
                {codePolicyStatus.parser} ABI{" "}
                {codePolicyStatus.languageAbiVersion}
              </dd>
            </div>
            <div>
              <dt>Compiled target</dt>
              <dd>{codePolicyStatus.compiledTarget}</dd>
            </div>
            <div>
              <dt>Filesystem</dt>
              <dd>{codePolicyStatus.sandboxRoot}</dd>
            </div>
            <div>
              <dt>Network</dt>
              <dd>{codePolicyStatus.networkAccess}</dd>
            </div>
            <div>
              <dt>Response</dt>
              <dd>{codePolicyStatus.responseContract}</dd>
            </div>
            <div>
              <dt>Grammars</dt>
              <dd>{codePolicyStatus.supportedLanguages.join(", ")}</dd>
            </div>
          </dl>
        ) : (
          <p role="status">Loading sandbox policy…</p>
        )}
      </section>
    </div>
  );
}
