import * as HoverCard from "@radix-ui/react-hover-card";
import * as ScrollArea from "@radix-ui/react-scroll-area";
import {
  type FormEvent,
  type KeyboardEvent,
  useEffect,
  useRef,
  useState,
} from "react";

import type { A2aStatus, BrowserStatus, ProxyStatus } from "../types";
import type { TraceEntry, UiChatMessage } from "../useBridgeState";

export type StatusSidebarProps = {
  proxyStatus: ProxyStatus | null;
  browserStatus: BrowserStatus | null;
  a2aStatus: A2aStatus | null;
  trace: TraceEntry[];
  chat: UiChatMessage[];
  sending: boolean;
  modelSelected: boolean;
  onSend: (prompt: string) => Promise<void>;
  onRetry: () => Promise<void>;
  onClearTrace: () => void;
  onClearChat: () => void;
};

function StatusIndicator({
  label,
  running,
  detail,
}: {
  label: string;
  running: boolean | null;
  detail: string;
}) {
  const state = running === null ? "unknown" : running ? "online" : "offline";
  return (
    <HoverCard.Root openDelay={250} closeDelay={80}>
      <HoverCard.Trigger asChild>
        <button
          className="status-indicator"
          type="button"
          data-state={state}
          aria-label={`${label}: ${state}. ${detail}`}
        >
          <span className="status-indicator-dot" aria-hidden="true" />
          <span>{label}</span>
          <span className="status-indicator-state">{state}</span>
        </button>
      </HoverCard.Trigger>
      <HoverCard.Portal>
        <HoverCard.Content className="status-hover-card" sideOffset={8}>
          <strong>{label}</strong>
          <p>{detail}</p>
          <HoverCard.Arrow className="status-hover-arrow" />
        </HoverCard.Content>
      </HoverCard.Portal>
    </HoverCard.Root>
  );
}

function formatTime(timestamp: Date): string {
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(timestamp);
}

function isNearBottom(element: HTMLElement): boolean {
  return element.scrollHeight - element.scrollTop - element.clientHeight < 64;
}

export function StatusSidebar({
  proxyStatus,
  browserStatus,
  a2aStatus,
  trace,
  chat,
  sending,
  modelSelected,
  onSend,
  onRetry,
  onClearTrace,
  onClearChat,
}: StatusSidebarProps) {
  const [prompt, setPrompt] = useState("");
  const [copiedMessageId, setCopiedMessageId] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);
  const traceViewport = useRef<HTMLDivElement>(null);
  const chatViewport = useRef<HTMLDivElement>(null);
  const tracePinned = useRef(true);
  const chatPinned = useRef(true);
  const copyTimer = useRef<number | null>(null);
  const latestTraceMessage = trace.at(-1)?.message;
  const latestChatContent = chat.at(-1)?.content;

  useEffect(() => {
    if (tracePinned.current && traceViewport.current) {
      traceViewport.current.scrollTop = traceViewport.current.scrollHeight;
    }
  }, [trace.length, latestTraceMessage]);

  useEffect(() => {
    if (chatPinned.current && chatViewport.current) {
      chatViewport.current.scrollTop = chatViewport.current.scrollHeight;
    }
  }, [chat.length, latestChatContent, sending]);

  useEffect(
    () => () => {
      if (copyTimer.current !== null) {
        window.clearTimeout(copyTimer.current);
      }
    },
    [],
  );

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const content = prompt.trim();
    if (!content || sending || !modelSelected) {
      return;
    }
    setPrompt("");
    chatPinned.current = true;
    void onSend(content);
  }

  function promptKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  }

  async function copyMessage(message: UiChatMessage) {
    if (!message.content) {
      return;
    }
    try {
      await navigator.clipboard.writeText(message.content);
      setCopyError(null);
      setCopiedMessageId(message.id);
      if (copyTimer.current !== null) {
        window.clearTimeout(copyTimer.current);
      }
      copyTimer.current = window.setTimeout(
        () => setCopiedMessageId(null),
        2_000,
      );
    } catch (error) {
      setCopyError(
        error instanceof Error ? error.message : "Clipboard access failed",
      );
    }
  }

  return (
    <aside
      id="agent-activity"
      className="status-sidebar"
      aria-label="Agent activity"
    >
      <header className="product-heading">
        <p className="eyebrow">Work mode</p>
        <h1>Bridge Codex</h1>
      </header>

      <section className="agent-status" aria-labelledby="agent-status-heading">
        <h2 id="agent-status-heading">Agent status</h2>
        <div className="status-list">
          <StatusIndicator
            label="Model router"
            running={proxyStatus?.running ?? null}
            detail={
              proxyStatus?.running
                ? "CLIProxyAPI is available on localhost:8317."
                : (proxyStatus?.error ?? "Waiting for CLIProxyAPI status.")
            }
          />
          <StatusIndicator
            label="Agent browser"
            running={browserStatus?.running ?? null}
            detail={
              browserStatus?.running
                ? `Chromium viewport is ${browserStatus.viewportWidth} × ${browserStatus.viewportHeight}.`
                : "The isolated browser starts only when requested."
            }
          />
          <StatusIndicator
            label="A2A endpoint"
            running={a2aStatus?.running ?? null}
            detail={
              a2aStatus?.running
                ? `Task delegation is available at ${a2aStatus.address}.`
                : (a2aStatus?.error ?? "Waiting for the local A2A server.")
            }
          />
        </div>
      </section>

      <section className="execution-trace" aria-labelledby="trace-heading">
        <div className="section-heading-row">
          <div>
            <h2 id="trace-heading">Execution trace</h2>
            <span>{trace.length} events</span>
          </div>
          <button
            type="button"
            disabled={trace.length === 0}
            onClick={onClearTrace}
          >
            Clear trace
          </button>
        </div>
        <ScrollArea.Root className="trace-scroll-area" type="auto">
          <ScrollArea.Viewport
            ref={traceViewport}
            className="trace-viewport"
            onScroll={(event) => {
              tracePinned.current = isNearBottom(event.currentTarget);
            }}
          >
            <ol>
              {trace.length === 0 ? (
                <li className="empty-state">Waiting for agent activity.</li>
              ) : (
                trace.map((entry) => (
                  <li key={entry.id} data-kind={entry.kind}>
                    <time dateTime={entry.timestamp.toISOString()}>
                      {formatTime(entry.timestamp)}
                    </time>
                    <span>{entry.message}</span>
                  </li>
                ))
              )}
            </ol>
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar orientation="vertical">
            <ScrollArea.Thumb />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
      </section>

      <section className="agent-chat" aria-labelledby="chat-heading">
        <div className="section-heading-row">
          <h2 id="chat-heading">Agent chat</h2>
          <div className="chat-actions">
            <button
              type="button"
              disabled={sending || chat.length === 0}
              onClick={() => void onRetry()}
            >
              Retry last
            </button>
            <button
              type="button"
              disabled={sending || chat.length === 0}
              onClick={onClearChat}
            >
              Clear chat
            </button>
          </div>
        </div>
        <ScrollArea.Root className="chat-scroll-area" type="auto">
          <ScrollArea.Viewport
            ref={chatViewport}
            className="chat-viewport"
            onScroll={(event) => {
              chatPinned.current = isNearBottom(event.currentTarget);
            }}
          >
            <div className="chat-message-list">
              {chat.length === 0 ? (
                <p className="empty-state">
                  Send a task to the selected model.
                </p>
              ) : (
                chat.map((message) => (
                  <article key={message.id} data-role={message.role}>
                    <header>
                      <span>{message.role === "user" ? "You" : "Agent"}</span>
                      {message.content && (
                        <button
                          type="button"
                          onClick={() => void copyMessage(message)}
                        >
                          {copiedMessageId === message.id ? "Copied" : "Copy"}
                        </button>
                      )}
                    </header>
                    {message.content && (
                      <pre className="chat-content">{message.content}</pre>
                    )}
                    {message.streaming && (
                      <p role="status">Receiving response…</p>
                    )}
                    {message.validation?.containsCode && (
                      <div
                        data-validation={
                          message.validation.valid ? "valid" : "invalid"
                        }
                      >
                        <p>
                          {message.validation.valid
                            ? `${message.validation.blocks.length} code block${message.validation.blocks.length === 1 ? "" : "s"} validated for the sandbox.`
                            : "The generated code does not satisfy the sandbox contract."}
                        </p>
                        {!message.validation.valid && (
                          <ul>
                            {message.validation.issues.map((issue, index) => (
                              <li
                                key={`${issue.code}-${issue.line ?? "unknown"}-${index}`}
                              >
                                {issue.line ? `Line ${issue.line}: ` : ""}
                                {issue.message}
                              </li>
                            ))}
                          </ul>
                        )}
                      </div>
                    )}
                    {message.error && <p role="alert">{message.error}</p>}
                  </article>
                ))
              )}
            </div>
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar orientation="vertical">
            <ScrollArea.Thumb />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
        {copyError && (
          <p role="alert">Could not copy the response: {copyError}</p>
        )}
        <p className="chat-progress" role="status" aria-live="polite">
          {sending ? "The selected model is working." : "Ready for a task."}
        </p>
        <form className="prompt-form" onSubmit={submit}>
          <label htmlFor="agent-prompt">Prompt</label>
          <textarea
            id="agent-prompt"
            value={prompt}
            onChange={(event) => setPrompt(event.currentTarget.value)}
            onKeyDown={promptKeyDown}
            placeholder={
              modelSelected ? "Describe the task…" : "Select a model first"
            }
            disabled={!modelSelected || sending}
            aria-describedby="prompt-shortcut"
            rows={4}
          />
          <p id="prompt-shortcut">Press Ctrl+Enter or Command+Enter to send.</p>
          <button
            type="submit"
            disabled={!prompt.trim() || !modelSelected || sending}
          >
            {sending ? "Working…" : "Send task"}
          </button>
        </form>
      </section>
    </aside>
  );
}
