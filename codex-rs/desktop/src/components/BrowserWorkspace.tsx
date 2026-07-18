import {
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  useEffect,
  useRef,
  useState,
} from "react";

import { useBrowserFrame } from "../browserFrameStore";
import type { BrowserStatus } from "../types";

export type BrowserWorkspaceProps = {
  status: BrowserStatus | null;
  busy: boolean;
  error?: string | null;
  onStart: () => Promise<void>;
  onStop: () => Promise<void>;
  onNavigate: (url: string) => Promise<void>;
  onClickSelector: (selector: string) => Promise<void>;
  onClickAt: (x: number, y: number) => Promise<void>;
  onType: (selector: string, text: string) => Promise<void>;
};

export function BrowserWorkspace({
  status,
  busy,
  error,
  onStart,
  onStop,
  onNavigate,
  onClickSelector,
  onClickAt,
  onType,
}: BrowserWorkspaceProps) {
  const frame = useBrowserFrame();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drawSequence = useRef(0);
  const editingAddress = useRef(false);
  const [address, setAddress] = useState(status?.url ?? "");
  const [selector, setSelector] = useState("");
  const [browserText, setBrowserText] = useState("");
  const [coordinateX, setCoordinateX] = useState("");
  const [coordinateY, setCoordinateY] = useState("");
  const [keyboardPoint, setKeyboardPoint] = useState({ x: 640, y: 360 });

  const viewportWidth = frame?.width ?? status?.viewportWidth ?? 1280;
  const viewportHeight = frame?.height ?? status?.viewportHeight ?? 720;
  const browserRunning = Boolean(status?.running);
  const previewInteractive = Boolean(frame && browserRunning && !busy);
  const health =
    status && "health" in status && typeof status.health === "string"
      ? status.health
      : browserRunning
        ? "running"
        : "stopped";
  const statusError =
    status && "error" in status && typeof status.error === "string"
      ? status.error
      : null;

  useEffect(() => {
    if (!editingAddress.current && status?.url) {
      setAddress(status.url);
    }
  }, [status?.url]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !frame) {
      return;
    }

    const sequence = drawSequence.current + 1;
    drawSequence.current = sequence;
    const image = new Image();
    image.onload = () => {
      if (sequence !== drawSequence.current) {
        return;
      }
      canvas.width = frame.width;
      canvas.height = frame.height;
      canvas
        .getContext("2d")
        ?.drawImage(image, 0, 0, frame.width, frame.height);
    };
    image.src = `data:image/jpeg;base64,${frame.jpegBase64}`;
  }, [frame]);

  useEffect(() => {
    setKeyboardPoint((point) => ({
      x: Math.min(Math.max(point.x, 0), Math.max(viewportWidth - 1, 0)),
      y: Math.min(Math.max(point.y, 0), Math.max(viewportHeight - 1, 0)),
    }));
  }, [viewportHeight, viewportWidth]);

  function submitNavigation(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const url = address.trim();
    if (!url) {
      return;
    }
    editingAddress.current = false;
    void onNavigate(url);
  }

  function submitSelectorClick(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = selector.trim();
    if (target) {
      void onClickSelector(target);
    }
  }

  function submitTyping(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const target = selector.trim();
    if (target && browserText) {
      void onType(target, browserText);
    }
  }

  function submitCoordinateClick(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (coordinateX === "" || coordinateY === "") {
      return;
    }
    const x = Number(coordinateX);
    const y = Number(coordinateY);
    if (
      Number.isFinite(x) &&
      Number.isFinite(y) &&
      x >= 0 &&
      x < viewportWidth &&
      y >= 0 &&
      y < viewportHeight
    ) {
      void onClickAt(x, y);
    }
  }

  function clickPreview(event: MouseEvent<HTMLCanvasElement>) {
    if (!frame || busy) {
      return;
    }
    const bounds = event.currentTarget.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0) {
      return;
    }
    const x = ((event.clientX - bounds.left) / bounds.width) * frame.width;
    const y = ((event.clientY - bounds.top) / bounds.height) * frame.height;
    void onClickAt(x, y);
  }

  function previewKeyDown(event: KeyboardEvent<HTMLCanvasElement>) {
    if (!previewInteractive) {
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      void onClickAt(keyboardPoint.x, keyboardPoint.y);
      return;
    }
    if (event.key === "Home") {
      event.preventDefault();
      setKeyboardPoint({
        x: Math.floor(viewportWidth / 2),
        y: Math.floor(viewportHeight / 2),
      });
      return;
    }
    const step = event.shiftKey ? 1 : 10;
    const movement =
      event.key === "ArrowLeft"
        ? { x: -step, y: 0 }
        : event.key === "ArrowRight"
          ? { x: step, y: 0 }
          : event.key === "ArrowUp"
            ? { x: 0, y: -step }
            : event.key === "ArrowDown"
              ? { x: 0, y: step }
              : null;
    if (!movement) {
      return;
    }
    event.preventDefault();
    setKeyboardPoint((point) => ({
      x: Math.min(Math.max(point.x + movement.x, 0), viewportWidth - 1),
      y: Math.min(Math.max(point.y + movement.y, 0), viewportHeight - 1),
    }));
  }

  return (
    <section
      className="browser-workspace"
      aria-label="Agent browser workspace"
      aria-busy={busy}
    >
      <form className="address-bar" onSubmit={submitNavigation}>
        <label htmlFor="browser-address">Web address</label>
        <input
          id="browser-address"
          type="text"
          inputMode="url"
          value={address}
          onChange={(event) => setAddress(event.currentTarget.value)}
          onFocus={() => {
            editingAddress.current = true;
          }}
          onBlur={() => {
            editingAddress.current = false;
          }}
          placeholder="https://example.com"
          autoComplete="url"
          spellCheck={false}
          disabled={busy}
        />
        <button type="submit" disabled={busy || !address.trim()}>
          Navigate
        </button>
        {browserRunning ? (
          <button type="button" disabled={busy} onClick={() => void onStop()}>
            Stop browser
          </button>
        ) : (
          <button type="button" disabled={busy} onClick={() => void onStart()}>
            {busy ? "Starting…" : "Start browser"}
          </button>
        )}
      </form>

      <div className="browser-meta" role="status" aria-live="polite">
        <span data-health={health}>{health}</span>
        <span title={frame?.url ?? status?.url ?? undefined}>
          {frame?.url ?? status?.url ?? "No page loaded"}
        </span>
        <span>
          {viewportWidth} × {viewportHeight}
        </span>
        {frame && (
          <span>
            Frame {frame.sequence} ·{" "}
            <time dateTime={frame.capturedAt}>
              {new Date(frame.capturedAt).toLocaleTimeString()}
            </time>
          </span>
        )}
      </div>

      {(error || statusError) && (
        <p className="workspace-error" role="alert">
          {error ?? statusError}
        </p>
      )}

      <div className="browser-canvas-shell">
        <canvas
          ref={canvasRef}
          width={viewportWidth}
          height={viewportHeight}
          onClick={clickPreview}
          onKeyDown={previewKeyDown}
          role="button"
          tabIndex={previewInteractive ? 0 : -1}
          aria-disabled={!previewInteractive}
          aria-describedby="browser-preview-help"
          aria-label={`Live agent browser preview${frame?.url ? ` of ${frame.url}` : ""}. Keyboard target ${Math.round(keyboardPoint.x)}, ${Math.round(keyboardPoint.y)}`}
        >
          Live browser preview at {viewportWidth} by {viewportHeight} pixels.
        </canvas>
        {frame && (
          <span
            className="browser-keyboard-target"
            style={{
              left: `${(keyboardPoint.x / viewportWidth) * 100}%`,
              top: `${(keyboardPoint.y / viewportHeight) * 100}%`,
            }}
            aria-hidden="true"
          />
        )}
        {!frame && (
          <div className="canvas-empty-state" role="status">
            <p>
              {browserRunning
                ? "Waiting for the first browser frame…"
                : "The isolated Chromium session is stopped."}
            </p>
            {!browserRunning && (
              <button
                type="button"
                disabled={busy}
                onClick={() => void onStart()}
              >
                Start browser
              </button>
            )}
          </div>
        )}
      </div>
      <p id="browser-preview-help">
        Click or tap the preview to interact by position. With keyboard focus,
        use the arrow keys to move the target and Enter to click. Exact controls
        are available below.
      </p>

      <details className="manual-tool-disclosure">
        <summary>Manual browser controls</summary>
        <p>
          Use these controls when you need to target an exact element or
          coordinate.
        </p>
        <div className="browser-tools">
          <form className="selector-tools" onSubmit={submitSelectorClick}>
            <label htmlFor="browser-selector">CSS selector</label>
            <input
              id="browser-selector"
              value={selector}
              onChange={(event) => setSelector(event.currentTarget.value)}
              placeholder="#search or [aria-label='Search']"
              spellCheck={false}
              disabled={!browserRunning || busy}
            />
            <button
              type="submit"
              disabled={!browserRunning || busy || !selector.trim()}
            >
              Click element
            </button>
          </form>

          <form className="browser-type-tools" onSubmit={submitTyping}>
            <label htmlFor="browser-type-text">
              Text to type into selected element
            </label>
            <textarea
              id="browser-type-text"
              value={browserText}
              onChange={(event) => setBrowserText(event.currentTarget.value)}
              rows={2}
              disabled={!browserRunning || busy}
            />
            <button
              type="submit"
              disabled={
                !browserRunning || busy || !selector.trim() || !browserText
              }
            >
              Type text
            </button>
          </form>

          <form className="coordinate-tools" onSubmit={submitCoordinateClick}>
            <fieldset disabled={!browserRunning || busy}>
              <legend>Viewport coordinate</legend>
              <label htmlFor="browser-coordinate-x">X</label>
              <input
                id="browser-coordinate-x"
                type="number"
                min={0}
                max={viewportWidth - 1}
                step={1}
                value={coordinateX}
                onChange={(event) => setCoordinateX(event.currentTarget.value)}
              />
              <label htmlFor="browser-coordinate-y">Y</label>
              <input
                id="browser-coordinate-y"
                type="number"
                min={0}
                max={viewportHeight - 1}
                step={1}
                value={coordinateY}
                onChange={(event) => setCoordinateY(event.currentTarget.value)}
              />
              <button
                type="submit"
                disabled={coordinateX === "" || coordinateY === ""}
              >
                Click coordinate
              </button>
            </fieldset>
          </form>
        </div>
      </details>
    </section>
  );
}
