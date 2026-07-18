import * as Dialog from "@radix-ui/react-dialog";
import * as HoverCard from "@radix-ui/react-hover-card";
import * as ScrollArea from "@radix-ui/react-scroll-area";
import {
  Activity,
  ArrowUp,
  Check,
  Copy,
  GitFork,
  Pencil,
  RotateCcw,
  Settings2,
  Square,
  Sparkles,
  Trash2,
} from "lucide-react";
import {
  type FormEvent,
  type KeyboardEvent,
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { conversationTitleFromPrompt } from "../conversationTitle";
import type { PendingServerRequest } from "../agentRuntimeReducer";
import type { RequestId } from "../../../app-server-protocol/schema/typescript/RequestId";
import type { ServerRequest } from "../../../app-server-protocol/schema/typescript/ServerRequest";
import type { A2aStatus, BrowserStatus, ProxyStatus } from "../types";
import type { TraceEntry, UiChatMessage } from "../workbenchTypes";
import { ServerRequestQueue } from "./ServerRequestQueue";

const MessageContent = lazy(async () => {
  const module = await import("./MessageContent");
  return { default: module.MessageContent };
});

export type StatusSidebarProps = {
  proxyStatus: ProxyStatus | null;
  browserStatus: BrowserStatus | null;
  a2aStatus: A2aStatus | null;
  trace: TraceEntry[];
  chat: UiChatMessage[];
  activeThreadName: string;
  hasActiveThread: boolean;
  pendingRequests: PendingServerRequest[];
  sending: boolean;
  selectedModel: string;
  onSend: (prompt: string) => Promise<void>;
  onRetry: () => Promise<void>;
  onInterrupt: () => Promise<void>;
  onForkThread: () => Promise<void>;
  onArchiveThread: () => Promise<void>;
  onNameThread: (name: string) => Promise<void>;
  onResolveRequest: (requestId: RequestId, result: unknown) => Promise<void>;
  onDenyRequest: (requestId: RequestId) => Promise<void>;
  onExecuteDynamicTool: (
    request: Extract<ServerRequest, { method: "item/tool/call" }>,
  ) => Promise<boolean>;
  onClearTrace: () => void;
  onOpenRouting: (trigger: HTMLButtonElement) => void;
};

function StatusIndicator({
  label,
  running,
  detail,
  disabled = false,
}: {
  label: string;
  running: boolean | null;
  detail: string;
  disabled?: boolean;
}) {
  const state = disabled
    ? "disabled"
    : running === null
      ? "unknown"
      : running
        ? "online"
        : "offline";
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

const ChatMessageArticle = memo(function ChatMessageArticle({
  message,
  copied,
  onCopy,
}: {
  message: UiChatMessage;
  copied: boolean;
  onCopy: (message: UiChatMessage) => void;
}) {
  return (
    <article data-role={message.role}>
      <header>
        <span>
          {message.label ?? (message.role === "user" ? "You" : "Bridge")}
        </span>
        {message.content && (
          <button
            className="message-copy-button"
            type="button"
            aria-label={`Copy ${message.role} message`}
            onClick={() => onCopy(message)}
          >
            {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
        )}
      </header>
      {message.content && (
        <Suspense
          fallback={
            <pre className="message-content message-fallback">
              {message.content}
            </pre>
          }
        >
          <MessageContent content={message.content} />
        </Suspense>
      )}
      {message.streaming && (
        <p className="streaming-status" role="status">
          <Sparkles aria-hidden="true" /> Receiving response…
        </p>
      )}
      {message.validation?.containsCode && (
        <div data-validation={message.validation.valid ? "valid" : "invalid"}>
          <p>
            {message.validation.valid
              ? `${message.validation.blocks.length} code block${message.validation.blocks.length === 1 ? "" : "s"} validated for the sandbox.`
              : "The generated code does not satisfy the sandbox contract."}
          </p>
          {!message.validation.valid && (
            <ul>
              {message.validation.issues.map((issue, index) => (
                <li key={`${issue.code}-${issue.line ?? "unknown"}-${index}`}>
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
  );
});

export const StatusSidebar = memo(function StatusSidebar({
  proxyStatus,
  browserStatus,
  a2aStatus,
  trace,
  chat,
  activeThreadName,
  hasActiveThread,
  pendingRequests,
  sending,
  selectedModel,
  onSend,
  onRetry,
  onInterrupt,
  onForkThread,
  onArchiveThread,
  onNameThread,
  onResolveRequest,
  onDenyRequest,
  onExecuteDynamicTool,
  onClearTrace,
  onOpenRouting,
}: StatusSidebarProps) {
  const [prompt, setPrompt] = useState("");
  const [copiedMessageId, setCopiedMessageId] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);
  const [clearConversationOpen, setClearConversationOpen] = useState(false);
  const [renameThreadOpen, setRenameThreadOpen] = useState(false);
  const [threadName, setThreadName] = useState("");
  const [completionAnnouncement, setCompletionAnnouncement] = useState("");
  const traceViewport = useRef<HTMLDivElement>(null);
  const chatViewport = useRef<HTMLDivElement>(null);
  const tracePinned = useRef(true);
  const chatPinned = useRef(true);
  const copyTimer = useRef<number | null>(null);
  const wasSending = useRef(sending);
  const latestTraceMessage = trace.at(-1)?.message;
  const latestChatContent = chat.at(-1)?.content;
  const modelSelected = Boolean(selectedModel);
  const firstPrompt = chat.find((message) => message.role === "user")?.content;
  const title = useMemo(
    () => activeThreadName || conversationTitleFromPrompt(firstPrompt),
    [activeThreadName, firstPrompt],
  );

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

  useEffect(() => {
    if (!wasSending.current && sending) {
      setCompletionAnnouncement("Bridge started responding.");
    } else if (wasSending.current && !sending) {
      setCompletionAnnouncement(
        chat.at(-1)?.error
          ? "Bridge response failed."
          : "Bridge response complete.",
      );
    }
    wasSending.current = sending;
  }, [chat, sending]);

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
    if (!content || !modelSelected) {
      return;
    }
    setPrompt("");
    chatPinned.current = true;
    void onSend(content);
  }

  function promptKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (
      event.key === "Enter" &&
      !event.shiftKey &&
      !event.nativeEvent.isComposing
    ) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
  }

  const copyMessage = useCallback(async (message: UiChatMessage) => {
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
  }, []);

  return (
    <main
      id="agent-activity"
      className="status-sidebar"
      aria-label="Current task"
    >
      <header className="thread-header">
        <div className="thread-heading">
          <p className="eyebrow">Local conversation</p>
          <h1>{title}</h1>
        </div>
        <div className="thread-header-actions">
          <div className="status-list" aria-label="Local service status">
            <StatusIndicator
              label="Router"
              running={proxyStatus?.running ?? null}
              detail={
                proxyStatus?.running
                  ? "CLIProxyAPI is available on localhost:8317."
                  : (proxyStatus?.error ?? "Waiting for CLIProxyAPI status.")
              }
            />
            <StatusIndicator
              label="Browser"
              running={browserStatus?.running ?? null}
              detail={
                browserStatus?.running
                  ? `Chromium viewport is ${browserStatus.viewportWidth} × ${browserStatus.viewportHeight}.`
                  : "The isolated browser starts only when requested."
              }
            />
            <StatusIndicator
              label="A2A"
              running={a2aStatus?.running ?? null}
              disabled={a2aStatus ? !a2aStatus.enabled : false}
              detail={
                a2aStatus && !a2aStatus.enabled
                  ? "Task delegation is disabled in this build's security configuration."
                  : a2aStatus?.running
                  ? `Task delegation is available at ${a2aStatus.address}.`
                  : (a2aStatus?.error ?? "Waiting for the local A2A server.")
              }
            />
          </div>
          <span className="thread-action-divider" aria-hidden="true" />
          <button
            className="icon-button"
            type="button"
            title="Retry the last prompt"
            aria-label="Retry the last prompt"
            disabled={sending || chat.length === 0}
            onClick={() => void onRetry()}
          >
            <RotateCcw aria-hidden="true" />
          </button>
          <button
            className="icon-button"
            type="button"
            title="Fork this thread"
            aria-label="Fork this thread"
            disabled={sending || !hasActiveThread}
            onClick={() => void onForkThread()}
          >
            <GitFork aria-hidden="true" />
          </button>
          <button
            className="icon-button"
            type="button"
            title="Rename this thread"
            aria-label="Rename this thread"
            disabled={!hasActiveThread}
            onClick={() => {
              setThreadName(activeThreadName || title);
              setRenameThreadOpen(true);
            }}
          >
            <Pencil aria-hidden="true" />
          </button>
          <button
            className="icon-button"
            type="button"
            title="Archive this thread"
            aria-label="Archive this thread"
            disabled={sending || !hasActiveThread}
            onClick={() => setClearConversationOpen(true)}
          >
            <Trash2 aria-hidden="true" />
          </button>
        </div>
      </header>

      <Dialog.Root
        open={clearConversationOpen}
        onOpenChange={setClearConversationOpen}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="dialog-overlay" />
          <Dialog.Content className="destructive-confirm-dialog">
            <Dialog.Title>Archive this thread?</Dialog.Title>
            <Dialog.Description>
              The app server will move this thread out of the active list. Its
              persisted rollout remains managed by Codex.
            </Dialog.Description>
            <div className="dialog-actions">
              <Dialog.Close asChild>
                <button type="button">Keep thread</button>
              </Dialog.Close>
              <button
                type="button"
                onClick={() => {
                  void onArchiveThread();
                  setClearConversationOpen(false);
                }}
              >
                Archive thread
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      <Dialog.Root open={renameThreadOpen} onOpenChange={setRenameThreadOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="dialog-overlay" />
          <Dialog.Content className="destructive-confirm-dialog">
            <Dialog.Title>Rename thread</Dialog.Title>
            <Dialog.Description>
              Choose the title shown in the Codex thread list.
            </Dialog.Description>
            <label>
              Thread name
              <input
                autoFocus
                type="text"
                value={threadName}
                onChange={(event) => setThreadName(event.currentTarget.value)}
              />
            </label>
            <div className="dialog-actions">
              <Dialog.Close asChild>
                <button type="button">Cancel</button>
              </Dialog.Close>
              <button
                type="button"
                disabled={!threadName.trim()}
                onClick={() => {
                  void onNameThread(threadName);
                  setRenameThreadOpen(false);
                }}
              >
                Save name
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      <section className="agent-chat" aria-label="Conversation">
        <ScrollArea.Root className="chat-scroll-area" type="auto">
          <ScrollArea.Viewport
            ref={chatViewport}
            className="chat-viewport"
            onScroll={(event) => {
              chatPinned.current = isNearBottom(event.currentTarget);
            }}
          >
            <div
              className="chat-message-list"
              role="log"
              aria-label="Conversation messages"
              aria-live="polite"
              aria-relevant="additions"
            >
              {chat.length === 0 ? (
                <div className="thread-empty-state">
                  <span className="bridge-mark" aria-hidden="true">
                    B
                  </span>
                  <div>
                    <h2>What are we building?</h2>
                    <p>
                      Start with a focused task. Bridge keeps the conversation
                      and local agent activity together.
                    </p>
                  </div>
                  <div className="prompt-suggestions" aria-label="Prompt ideas">
                    <button
                      type="button"
                      onClick={() => setPrompt("Help me plan a coding task")}
                    >
                      Plan a coding task
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        setPrompt("Draft a safe implementation plan")
                      }
                    >
                      Draft an implementation
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        setPrompt("Explain this code and its tradeoffs")
                      }
                    >
                      Explain code
                    </button>
                  </div>
                </div>
              ) : (
                chat.map((message) => (
                  <ChatMessageArticle
                    key={message.id}
                    message={message}
                    copied={copiedMessageId === message.id}
                    onCopy={copyMessage}
                  />
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
        <p className="visually-hidden" role="status" aria-live="polite">
          {completionAnnouncement}
        </p>
      </section>

      <ServerRequestQueue
        requests={pendingRequests}
        onResolve={onResolveRequest}
        onDeny={onDenyRequest}
        onExecuteDynamicTool={onExecuteDynamicTool}
      />

      <footer className="thread-footer">
        <details className="execution-trace">
          <summary>
            <span>
              <Activity aria-hidden="true" /> Activity
            </span>
            <span>{trace.length} events</span>
          </summary>
          <div className="trace-panel">
            <div className="trace-panel-header">
              <p>Local service and execution events</p>
              <button
                type="button"
                disabled={trace.length === 0}
                onClick={onClearTrace}
              >
                Clear
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
          </div>
        </details>

        <form className="prompt-form" onSubmit={submit}>
          <label className="visually-hidden" htmlFor="agent-prompt">
            Message Bridge Codex
          </label>
          <textarea
            id="agent-prompt"
            value={prompt}
            onChange={(event) => setPrompt(event.currentTarget.value)}
            onKeyDown={promptKeyDown}
            placeholder={
              modelSelected
                ? "Ask Bridge to work on something…"
                : "Choose a model to begin"
            }
            disabled={!modelSelected}
            rows={3}
          />
          <div className="composer-toolbar">
            <button
              className="model-pill"
              type="button"
              onClick={(event) => onOpenRouting(event.currentTarget)}
            >
              <Settings2 aria-hidden="true" />
              <span>{selectedModel || "Choose model"}</span>
            </button>
            <div className="composer-status">
              <span role="status" aria-live="polite">
                {sending
                  ? "Send to steer the active turn"
                  : "Enter to send · Shift+Enter for a new line"}
              </span>
              {sending && (
                <button
                  className="interrupt-button"
                  type="button"
                  aria-label="Interrupt active turn"
                  title="Interrupt active turn"
                  onClick={() => void onInterrupt()}
                >
                  <Square aria-hidden="true" />
                </button>
              )}
              <button
                className="send-button"
                type="submit"
                aria-label={sending ? "Steer active turn" : "Send message"}
                disabled={!prompt.trim() || !modelSelected}
              >
                <ArrowUp aria-hidden="true" />
              </button>
            </div>
          </div>
        </form>
      </footer>
    </main>
  );
});
