import * as Dialog from "@radix-ui/react-dialog";
import { type FormEvent, useState } from "react";

import type { DesktopStatus } from "../types";

export type DesktopWorkspaceProps = {
  status: DesktopStatus | null;
  busy: boolean;
  error?: string | null;
  onRefresh: () => Promise<void>;
  onEnable: () => Promise<void>;
  onDisable: () => Promise<void>;
  onClickAt: (x: number, y: number) => Promise<void>;
  onType: (text: string) => Promise<void>;
};

export function DesktopWorkspace({
  status,
  busy,
  error,
  onRefresh,
  onEnable,
  onDisable,
  onClickAt,
  onType,
}: DesktopWorkspaceProps) {
  const [enableOpen, setEnableOpen] = useState(false);
  const [coordinateX, setCoordinateX] = useState("");
  const [coordinateY, setCoordinateY] = useState("");
  const [text, setText] = useState("");

  const enabled = Boolean(status && "enabled" in status && status.enabled);
  const available = Boolean(status?.available);
  const displayWidth = status?.displaySize?.[0];
  const displayHeight = status?.displaySize?.[1];

  async function enableWorkMode() {
    await onEnable();
    setEnableOpen(false);
  }

  function submitClick(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (coordinateX === "" || coordinateY === "") {
      return;
    }
    const x = Number(coordinateX);
    const y = Number(coordinateY);
    if (
      Number.isInteger(x) &&
      Number.isInteger(y) &&
      x >= 0 &&
      y >= 0 &&
      (displayWidth === undefined || x < displayWidth) &&
      (displayHeight === undefined || y < displayHeight)
    ) {
      void onClickAt(x, y);
    }
  }

  function submitText(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (text) {
      void onType(text);
    }
  }

  return (
    <section
      className="desktop-workspace"
      aria-label="Desktop control workspace"
      aria-busy={busy}
    >
      <header>
        <div>
          <p className="eyebrow">Host controller</p>
          <h2>Desktop control</h2>
          <p id="desktop-safety-note">
            Actions move the real pointer and type into the currently focused
            host application. Work Mode stays disabled until you explicitly
            enable it.
          </p>
        </div>
        <button type="button" disabled={busy} onClick={() => void onRefresh()}>
          Refresh status
        </button>
      </header>

      <dl className="desktop-status-grid">
        <div>
          <dt>Controller</dt>
          <dd>
            {status === null
              ? "Checking…"
              : available
                ? "Available"
                : "Unavailable"}
          </dd>
        </div>
        <div>
          <dt>Work Mode</dt>
          <dd>{enabled ? "Enabled" : "Disabled"}</dd>
        </div>
        <div>
          <dt>Platform</dt>
          <dd>{status?.platform ?? "Checking…"}</dd>
        </div>
        <div>
          <dt>Main display</dt>
          <dd>
            {displayWidth && displayHeight
              ? `${displayWidth} × ${displayHeight}`
              : "Not reported"}
          </dd>
        </div>
      </dl>

      {(error || status?.error) && (
        <p className="workspace-error" role="alert">
          {error ?? status?.error}
        </p>
      )}

      <div className="desktop-gate" aria-describedby="desktop-safety-note">
        {enabled ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => void onDisable()}
          >
            {busy ? "Disabling…" : "Disable desktop Work Mode"}
          </button>
        ) : (
          <Dialog.Root open={enableOpen} onOpenChange={setEnableOpen}>
            <Dialog.Trigger asChild>
              <button type="button" disabled={!available || busy}>
                Enable desktop Work Mode
              </button>
            </Dialog.Trigger>
            <Dialog.Portal>
              <Dialog.Overlay className="dialog-overlay" />
              <Dialog.Content className="desktop-confirm-dialog">
                <Dialog.Title>Enable control of this desktop?</Dialog.Title>
                <Dialog.Description>
                  Bridge Codex will be able to move the real mouse pointer and
                  type into the focused application until you disable Work Mode
                  or close the app.
                </Dialog.Description>
                <div className="dialog-actions">
                  <Dialog.Close asChild>
                    <button type="button">Keep disabled</button>
                  </Dialog.Close>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void enableWorkMode()}
                  >
                    {busy ? "Enabling…" : "Enable control"}
                  </button>
                </div>
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
        )}
      </div>

      <div className="desktop-tools">
        <form onSubmit={submitClick}>
          <fieldset disabled={!enabled || busy}>
            <legend>Absolute pointer click</legend>
            <label htmlFor="desktop-coordinate-x">X coordinate</label>
            <input
              id="desktop-coordinate-x"
              type="number"
              min={0}
              max={displayWidth ? displayWidth - 1 : undefined}
              step={1}
              value={coordinateX}
              onChange={(event) => setCoordinateX(event.currentTarget.value)}
            />
            <label htmlFor="desktop-coordinate-y">Y coordinate</label>
            <input
              id="desktop-coordinate-y"
              type="number"
              min={0}
              max={displayHeight ? displayHeight - 1 : undefined}
              step={1}
              value={coordinateY}
              onChange={(event) => setCoordinateY(event.currentTarget.value)}
            />
            <button
              type="submit"
              disabled={coordinateX === "" || coordinateY === ""}
            >
              Click desktop
            </button>
          </fieldset>
        </form>

        <form onSubmit={submitText}>
          <fieldset disabled={!enabled || busy}>
            <legend>Native keyboard input</legend>
            <label htmlFor="desktop-type-text">Text to type</label>
            <textarea
              id="desktop-type-text"
              rows={4}
              value={text}
              onChange={(event) => setText(event.currentTarget.value)}
              placeholder="Text is sent to the currently focused application"
            />
            <button type="submit" disabled={!text}>
              Type on desktop
            </button>
          </fieldset>
        </form>
      </div>
    </section>
  );
}
