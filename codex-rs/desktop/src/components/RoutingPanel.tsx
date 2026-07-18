import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { type Dispatch, type FormEvent, useEffect, useState } from "react";

import type { BridgeAgentRuntimeStatus } from "../agentRuntime";
import type {
  LocalDataAction,
  LocalDataState,
} from "../localDataReducer";
import type { CodePolicyStatus, ProxyModel, ProxyStatus } from "../types";
import { LocalDataPanel } from "./LocalDataPanel";

export type RoutingPanelProps = {
  proxyStatus: ProxyStatus | null;
  codePolicyStatus: CodePolicyStatus | null;
  models: ProxyModel[];
  selectedModel: string;
  runtimeStatus: BridgeAgentRuntimeStatus | null;
  busy: boolean;
  error?: string | null;
  onSelectModel: (model: string) => void;
  onRefreshModels: () => Promise<void>;
  onConfigureProxy: (baseUrl: string, apiKey: string) => Promise<void>;
  localDataState: LocalDataState;
  dispatchLocalData: Dispatch<LocalDataAction>;
  conversationCount: number;
  currentMessageCount: number;
  localDataBusy: boolean;
  onExportSession: () => void;
  onArchiveCurrent: () => void;
  onDeleteAllLocalData: () => Promise<boolean>;
  onExportSupportBundle: () => Promise<string>;
};

export function RoutingPanel({
  proxyStatus,
  codePolicyStatus,
  models,
  selectedModel,
  runtimeStatus,
  busy,
  error,
  onSelectModel,
  onRefreshModels,
  onConfigureProxy,
  localDataState,
  dispatchLocalData,
  conversationCount,
  currentMessageCount,
  localDataBusy,
  onExportSession,
  onArchiveCurrent,
  onDeleteAllLocalData,
  onExportSupportBundle,
}: RoutingPanelProps) {
  const [baseUrl, setBaseUrl] = useState(
    proxyStatus?.baseUrl || "http://127.0.0.1:8317/",
  );
  const [apiKey, setApiKey] = useState("");
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const selected = models.find((model) => model.id === selectedModel);
  const probedModel = proxyStatus?.probedModel ?? null;

  useEffect(() => {
    if (proxyStatus?.baseUrl) {
      setBaseUrl(proxyStatus.baseUrl);
    }
  }, [proxyStatus?.baseUrl]);

  async function connectProxy(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const url = new URL(baseUrl);
      const loopback =
        url.hostname.toLowerCase() === "localhost" ||
        url.hostname.startsWith("127.") ||
        url.hostname === "[::1]";
      if (
        url.protocol !== "http:" ||
        !loopback ||
        url.username ||
        url.password ||
        url.pathname !== "/" ||
        url.search ||
        url.hash
      ) {
        throw new Error(
          "Use an HTTP loopback origin without credentials, path, query, or fragment.",
        );
      }
      if (!apiKey.trim()) {
        throw new Error("Enter the proxy API key.");
      }
      setConnectionError(null);
      await onConfigureProxy(url.toString(), apiKey);
    } catch (connectionFailure) {
      setConnectionError(
        connectionFailure instanceof Error
          ? connectionFailure.message
          : "The proxy connection could not be configured.",
      );
    } finally {
      setApiKey("");
    }
  }

  function modelSafetyLabel(model: ProxyModel): string {
    const classification =
      model.classification === "known" ? "Known" : "Experimental";
    return model.id === probedModel
      ? `${classification} · conformance tested`
      : `${classification} · unavailable · not conformance tested`;
  }

  return (
    <div className="routing-panel" aria-busy={busy}>
      <section className="model-control" aria-labelledby="model-heading">
        <div className="section-heading-row">
          <div>
            <h3 id="model-heading">Active model</h3>
            <p>Choose the model used for conversations and delegated tasks.</p>
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
                    <DropdownMenu.RadioItem
                      key={model.id}
                      value={model.id}
                      disabled={model.id !== probedModel}
                    >
                      <DropdownMenu.ItemIndicator aria-hidden="true">
                        ●
                      </DropdownMenu.ItemIndicator>
                      <span>{model.id}</span>
                      <small>{modelSafetyLabel(model)}</small>
                    </DropdownMenu.RadioItem>
                  ))}
                </DropdownMenu.RadioGroup>
              )}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>

        {models.length === 0 && !busy && (
          <p className="empty-state">
            No authenticated models are available. Connect a compatible
            loopback proxy first.
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
                <dt>Classification</dt>
                <dd>{modelSafetyLabel(selected)}</dd>
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
        <h3 id="connection-heading">Loopback model proxy</h3>
        {proxyStatus?.authenticated && proxyStatus.responsesApi ? (
          <div className="connection-locked" role="status">
            <strong>Connected and locked</strong>
            <p>
              Bridge is using the configured loopback origin. The API key is
              held by the native credential store and is never returned to this
              interface.
            </p>
          </div>
        ) : (
          <form className="proxy-connect-form" onSubmit={connectProxy}>
            <p>
              First-run setup accepts only an HTTP loopback origin. Bridge
              passes the key directly to the native credential store and clears
              this field after the request.
            </p>
            <label>
              Loopback base URL
              <input
                type="url"
                value={baseUrl}
                spellCheck={false}
                autoComplete="off"
                onChange={(event) => setBaseUrl(event.currentTarget.value)}
              />
            </label>
            <label>
              Proxy API key
              <input
                type="password"
                value={apiKey}
                autoComplete="new-password"
                onChange={(event) => setApiKey(event.currentTarget.value)}
              />
            </label>
            <button type="submit" disabled={busy || !apiKey.trim()}>
              {busy ? "Connecting…" : "Connect proxy"}
            </button>
          </form>
        )}
        {models.length > 0 && (
          <p className="model-availability-note">
            Only {probedModel ?? "the model that passes setup"} is available
            for RC1. Other reported identifiers remain visible but cannot run
            until they pass the Responses and tool-call conformance probe.
          </p>
        )}
        {(connectionError || proxyStatus?.error) && (
          <p role="alert">{connectionError ?? proxyStatus?.error}</p>
        )}
        {proxyStatus?.probedModel && (
          <p>Responses probe: {proxyStatus.probedModel}</p>
        )}
        {Boolean(proxyStatus?.experimentalModelCount) && (
          <p>
            {proxyStatus?.experimentalModelCount} experimental model
            {proxyStatus?.experimentalModelCount === 1 ? " identifier" : " identifiers"} reported.
            Unprobed identifiers remain disabled.
          </p>
        )}
      </section>

      {error && <p role="alert">{error}</p>}

      <section className="runtime-safety" aria-labelledby="runtime-heading">
        <h3 id="runtime-heading">Live app-server runtime</h3>
        <dl>
          <div>
            <dt>State</dt>
            <dd>{runtimeStatus?.running ? "Running" : "Not running"}</dd>
          </div>
          <div>
            <dt>Filesystem</dt>
            <dd>workspace-write</dd>
          </div>
          <div>
            <dt>Network</dt>
            <dd>Denied by default</dd>
          </div>
          <div>
            <dt>Approvals</dt>
            <dd>User mediated</dd>
          </div>
        </dl>
        {runtimeStatus?.error && <p role="alert">{runtimeStatus.error}</p>}
      </section>

      <details className="sandbox-control">
        <summary id="sandbox-heading">Static code-policy preview only</summary>
        <p>
          This parser contract is informational and does not describe the live
          app-server sandbox shown above.
        </p>
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
      </details>

      <LocalDataPanel
        state={localDataState}
        dispatch={dispatchLocalData}
        conversationCount={conversationCount}
        currentMessageCount={currentMessageCount}
        busy={localDataBusy}
        onExportSession={onExportSession}
        onArchiveCurrent={onArchiveCurrent}
        onDeleteAll={onDeleteAllLocalData}
        onExportSupportBundle={onExportSupportBundle}
      />
    </div>
  );
}
