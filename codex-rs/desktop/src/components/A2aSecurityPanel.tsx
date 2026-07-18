import { type FormEvent, useEffect, useState } from "react";

import type { A2aStatus } from "../types";

export type A2aSecurityPanelProps = {
  status: A2aStatus | null;
  busy: boolean;
  onConfigure: (enabled: boolean, port: number) => Promise<unknown>;
  onProvisionToken: (regenerate: boolean) => Promise<string | null>;
  onDeleteToken: () => Promise<unknown>;
};

function statusPort(status: A2aStatus | null): number {
  const match = status?.address.match(/:(\d+)$/u);
  const port = Number(match?.[1]);
  return Number.isInteger(port) && port > 0 && port <= 65_535 ? port : 8120;
}

export function A2aSecurityPanel({
  status,
  busy,
  onConfigure,
  onProvisionToken,
  onDeleteToken,
}: A2aSecurityPanelProps) {
  const [enabled, setEnabled] = useState(status?.enabled ?? false);
  const [port, setPort] = useState(statusPort(status));
  const [revealedToken, setRevealedToken] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState("");

  useEffect(() => {
    if (status) {
      setEnabled(status.enabled);
      setPort(statusPort(status));
    }
  }, [status]);

  async function configure(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!Number.isInteger(port) || port < 1 || port > 65_535) {
      return;
    }
    await onConfigure(enabled, port);
  }

  async function provision(regenerate: boolean) {
    setRevealedToken(null);
    setCopyStatus("");
    const token = await onProvisionToken(regenerate);
    if (token) {
      setRevealedToken(token);
    }
  }

  return (
    <section className="a2a-security" aria-labelledby="a2a-security-heading">
      <div className="section-heading-row">
        <div>
          <h3 id="a2a-security-heading">Task-service security</h3>
          <p>
            The A2A listener binds to loopback. A bearer token is required when
            the service is enabled.
          </p>
        </div>
        <span data-state={status?.running ? "online" : "offline"}>
          {status?.running ? "Running" : status?.enabled ? "Stopped" : "Disabled"}
        </span>
      </div>

      <form className="a2a-security-form" onSubmit={configure}>
        <label>
          <input
            type="checkbox"
            checked={enabled}
            onChange={(event) => setEnabled(event.currentTarget.checked)}
          />
          Enable loopback A2A server
        </label>
        <label>
          Loopback port
          <input
            type="number"
            min={1}
            max={65_535}
            value={port}
            onChange={(event) => setPort(event.currentTarget.valueAsNumber)}
          />
        </label>
        <button type="submit" disabled={busy || !Number.isInteger(port)}>
          Save A2A settings
        </button>
      </form>

      <div className="a2a-token-control">
        <p>
          Token: {status?.tokenConfigured ? "configured" : "not configured"}
        </p>
        <div>
          <button
            type="button"
            disabled={busy}
            onClick={() => void provision(Boolean(status?.tokenConfigured))}
          >
            {status?.tokenConfigured ? "Regenerate token" : "Generate token"}
          </button>
          {status?.tokenConfigured && (
            <button
              type="button"
              disabled={busy}
              onClick={() => {
                setRevealedToken(null);
                void onDeleteToken();
              }}
            >
              Delete token
            </button>
          )}
        </div>
      </div>

      {revealedToken && (
        <div className="a2a-token-once" role="status">
          <strong>Copy this token now. It will not be shown again.</strong>
          <code>{revealedToken}</code>
          <div>
            <button
              type="button"
              onClick={() => {
                void navigator.clipboard
                  .writeText(revealedToken)
                  .then(() => setCopyStatus("Copied"))
                  .catch(() => setCopyStatus("Copy failed"));
              }}
            >
              Copy token
            </button>
            <button type="button" onClick={() => setRevealedToken(null)}>
              Dismiss
            </button>
            {copyStatus && <span>{copyStatus}</span>}
          </div>
        </div>
      )}
      {status?.error && <p role="alert">{status.error}</p>}
    </section>
  );
}
