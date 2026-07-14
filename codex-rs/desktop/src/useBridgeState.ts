import { useCallback, useEffect, useRef, useState } from "react";

import { backend } from "./backend";
import type {
  A2aStatus,
  A2aTask,
  BrowserFrame,
  BrowserStatus,
  ChatChunk,
  ChatMessage,
  CodeMemorySearchResult,
  CodeMemoryStatus,
  CodePolicyStatus,
  CodeValidation,
  DelegateA2aTaskRequest,
  DesktopStatus,
  LoginLaunch,
  ProxyModel,
  ProxyStatus,
  SandboxValidationEvent,
} from "./types";

const SESSION_STORAGE_KEY = "bridge-codex.session.v1";
const MAX_TRACE_ENTRIES = 200;
const MAX_TRACE_MESSAGE_LENGTH = 4_096;
const MAX_CHAT_MESSAGES = 96;
const MAX_CHAT_STATE_LENGTH = 2 * 1024 * 1024;
const MAX_PERSISTED_CHAT_LENGTH = 512 * 1024;
const MAX_CHAT_REQUEST_LENGTH = 60 * 1024;
const MAX_CHAT_REQUEST_MESSAGES = 120;
const LOGIN_POLL_ATTEMPTS = 60;
const LOGIN_POLL_INTERVAL_MS = 2_000;

export type TraceEntry = {
  id: string;
  timestamp: Date;
  kind: "info" | "success" | "error";
  message: string;
};

export type UiChatMessage = ChatMessage & {
  id: string;
  streaming?: boolean;
  error?: string | null;
  validation?: CodeValidation;
};

export type LoginState =
  | { phase: "idle" }
  | { phase: "launching" }
  | { phase: "launched"; launch: LoginLaunch }
  | { phase: "error"; error: string };

export type LoginVerificationState =
  | { phase: "idle" }
  | { phase: "checking"; attempt: number }
  | { phase: "verified"; modelCount: number; verifiedAt: string }
  | { phase: "pending"; error: string };

export type BridgeLoadingState = {
  initial: boolean;
  proxy: boolean;
  models: boolean;
  browser: boolean;
  desktop: boolean;
  a2a: boolean;
  codeMemory: boolean;
  chat: boolean;
  login: boolean;
};

export type BridgeErrorState = {
  initialization: string | null;
  proxy: string | null;
  models: string | null;
  browser: string | null;
  desktop: string | null;
  a2a: string | null;
  codeMemory: string | null;
  chat: string | null;
  login: string | null;
};

type LoadingResource = Exclude<keyof BridgeLoadingState, "initial">;
type ErrorResource = Exclude<keyof BridgeErrorState, "initialization">;

type BridgeEventSubscriber = {
  proxyStatus: (status: ProxyStatus) => void;
  a2aStatus: (status: A2aStatus) => void;
  chatChunk: (chunk: ChatChunk) => void;
  sandboxValidation: (event: SandboxValidationEvent) => void;
  browserFrame: (frame: BrowserFrame) => void;
  codeMemoryStatus: (status: CodeMemoryStatus) => void;
  bridgeError: (error: unknown) => void;
};

type InitialSnapshot = {
  proxy: PromiseSettledResult<ProxyStatus>;
  browser: PromiseSettledResult<BrowserStatus>;
  desktop: PromiseSettledResult<DesktopStatus>;
  a2a: PromiseSettledResult<A2aStatus>;
  codePolicy: PromiseSettledResult<CodePolicyStatus>;
  codeMemory: PromiseSettledResult<CodeMemoryStatus>;
};

type PersistedSession = {
  selectedModel: string;
  trace: TraceEntry[];
  chat: UiChatMessage[];
};

const eventSubscribers = new Set<BridgeEventSubscriber>();
let eventBridgePromise: Promise<void> | null = null;
let initialSnapshotPromise: Promise<InitialSnapshot> | null = null;
let modelRequestPromise: Promise<ProxyModel[]> | null = null;
let taskListRequestPromise: Promise<A2aTask[]> | null = null;
let localId = 0;

function nextId(prefix: string): string {
  localId += 1;
  return `${prefix}-${Date.now()}-${localId}`;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  try {
    return JSON.stringify(error) || String(error);
  } catch {
    return String(error);
  }
}

function truncateText(value: string, maxLength: number): string {
  if (value.length <= maxLength) {
    return value;
  }
  let end = Math.max(0, maxLength - 1);
  const finalCodeUnit = value.charCodeAt(end - 1);
  if (finalCodeUnit >= 0xd800 && finalCodeUnit <= 0xdbff) {
    end -= 1;
  }
  return `${value.slice(0, end)}…`;
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function boundedTrace(entries: TraceEntry[]): TraceEntry[] {
  return entries.slice(-MAX_TRACE_ENTRIES);
}

function boundedChat(
  messages: UiChatMessage[],
  maxLength = MAX_CHAT_STATE_LENGTH,
): UiChatMessage[] {
  const retained: UiChatMessage[] = [];
  let length = 0;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (!message) {
      continue;
    }
    const nextLength =
      length + message.content.length + (message.error?.length ?? 0);
    if (retained.length >= MAX_CHAT_MESSAGES || nextLength > maxLength) {
      break;
    }
    retained.push(message);
    length = nextLength;
  }
  retained.reverse();
  return retained;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function loadPersistedSession(): PersistedSession {
  const empty: PersistedSession = { selectedModel: "", trace: [], chat: [] };
  if (typeof window === "undefined") {
    return empty;
  }
  try {
    const raw = window.localStorage.getItem(SESSION_STORAGE_KEY);
    if (!raw) {
      return empty;
    }
    const value: unknown = JSON.parse(raw);
    if (!isRecord(value)) {
      return empty;
    }
    const selectedModel =
      typeof value.selectedModel === "string"
        ? value.selectedModel.slice(0, 256)
        : "";
    const trace = Array.isArray(value.trace)
      ? value.trace.flatMap((candidate): TraceEntry[] => {
          if (
            !isRecord(candidate) ||
            typeof candidate.id !== "string" ||
            typeof candidate.timestamp !== "string" ||
            typeof candidate.message !== "string" ||
            !["info", "success", "error"].includes(String(candidate.kind))
          ) {
            return [];
          }
          const timestamp = new Date(candidate.timestamp);
          if (Number.isNaN(timestamp.getTime())) {
            return [];
          }
          return [
            {
              id: candidate.id,
              timestamp,
              kind: candidate.kind as TraceEntry["kind"],
              message: truncateText(
                candidate.message,
                MAX_TRACE_MESSAGE_LENGTH,
              ),
            },
          ];
        })
      : [];
    const chat = Array.isArray(value.chat)
      ? value.chat.flatMap((candidate): UiChatMessage[] => {
          if (
            !isRecord(candidate) ||
            typeof candidate.id !== "string" ||
            typeof candidate.content !== "string" ||
            !["system", "user", "assistant"].includes(String(candidate.role))
          ) {
            return [];
          }
          const interrupted = candidate.streaming === true;
          return [
            {
              id: candidate.id,
              role: candidate.role as ChatMessage["role"],
              content: candidate.content,
              streaming: false,
              error: interrupted
                ? "The previous response stream was interrupted when the app closed."
                : typeof candidate.error === "string"
                  ? candidate.error
                  : null,
            },
          ];
        })
      : [];
    return {
      selectedModel,
      trace: boundedTrace(trace),
      chat: boundedChat(chat, MAX_PERSISTED_CHAT_LENGTH),
    };
  } catch {
    return empty;
  }
}

function persistSession(session: PersistedSession): void {
  try {
    window.localStorage.setItem(
      SESSION_STORAGE_KEY,
      JSON.stringify({
        selectedModel: session.selectedModel,
        trace: boundedTrace(session.trace).map((entry) => ({
          ...entry,
          timestamp: entry.timestamp.toISOString(),
        })),
        chat: boundedChat(session.chat, MAX_PERSISTED_CHAT_LENGTH).map(
          ({ id, role, content, streaming, error }) => ({
            id,
            role,
            content,
            streaming,
            error,
          }),
        ),
      }),
    );
  } catch {
    // Persistence is a best-effort convenience; Tauri may disable web storage.
  }
}

function requestInitialSnapshot(): Promise<InitialSnapshot> {
  if (!initialSnapshotPromise) {
    initialSnapshotPromise = Promise.allSettled([
      backend.getProxyStatus(),
      backend.getBrowserStatus(),
      backend.getDesktopStatus(),
      backend.getA2aStatus(),
      backend.getCodePolicyStatus(),
      backend.getCodeMemoryStatus(),
    ]).then(([proxy, browser, desktop, a2a, codePolicy, codeMemory]) => ({
      proxy,
      browser,
      desktop,
      a2a,
      codePolicy,
      codeMemory,
    }));
  }
  return initialSnapshotPromise;
}

function fetchModelsShared(): Promise<ProxyModel[]> {
  if (!modelRequestPromise) {
    const request = backend.fetchModels();
    modelRequestPromise = request;
    const clearRequest = () => {
      if (modelRequestPromise === request) {
        modelRequestPromise = null;
      }
    };
    void request.then(clearRequest, clearRequest);
  }
  return modelRequestPromise;
}

function fetchTasksShared(): Promise<A2aTask[]> {
  if (!taskListRequestPromise) {
    const request = backend.listA2aTasks();
    taskListRequestPromise = request;
    const clearRequest = () => {
      if (taskListRequestPromise === request) {
        taskListRequestPromise = null;
      }
    };
    void request.then(clearRequest, clearRequest);
  }
  return taskListRequestPromise;
}

function emitToSubscribers<Key extends keyof BridgeEventSubscriber>(
  key: Key,
  payload: Parameters<BridgeEventSubscriber[Key]>[0],
): void {
  for (const subscriber of eventSubscribers) {
    const handler = subscriber[key] as (value: typeof payload) => void;
    handler(payload);
  }
}

function ensureEventBridge(): Promise<void> {
  if (!eventBridgePromise) {
    eventBridgePromise = Promise.all([
      backend.onProxyStatus((status) =>
        emitToSubscribers("proxyStatus", status),
      ),
      backend.onA2aStatus((status) => emitToSubscribers("a2aStatus", status)),
      backend.onChatChunk((chunk) => emitToSubscribers("chatChunk", chunk)),
      backend.onSandboxValidation((event) =>
        emitToSubscribers("sandboxValidation", event),
      ),
      backend.onBrowserFrame((frame) =>
        emitToSubscribers("browserFrame", frame),
      ),
      backend.onCodeMemoryStatus((status) =>
        emitToSubscribers("codeMemoryStatus", status),
      ),
    ])
      .then(() => undefined)
      .catch((error: unknown) => {
        emitToSubscribers("bridgeError", error);
        throw error;
      });
  }
  return eventBridgePromise;
}

function subscribeBridgeEvents(subscriber: BridgeEventSubscriber): () => void {
  eventSubscribers.add(subscriber);
  void ensureEventBridge().catch(() => {
    // The shared bridge already forwards the actionable error to subscribers.
  });
  return () => {
    eventSubscribers.delete(subscriber);
  };
}

function requestHistory(
  messages: UiChatMessage[],
  maxLength: number,
): ChatMessage[] {
  const history: ChatMessage[] = [];
  let totalLength = 0;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (
      !message ||
      !message.content.trim() ||
      message.error ||
      message.streaming
    ) {
      continue;
    }
    const messageLength = utf8Length(message.content);
    if (
      history.length >= MAX_CHAT_REQUEST_MESSAGES ||
      totalLength + messageLength > maxLength
    ) {
      break;
    }
    history.push({ role: message.role, content: message.content });
    totalLength += messageLength;
  }
  history.reverse();
  return history;
}

function replaceTask(tasks: A2aTask[], task: A2aTask): A2aTask[] {
  const index = tasks.findIndex((candidate) => candidate.id === task.id);
  if (index < 0) {
    return [task, ...tasks];
  }
  return tasks.map((candidate) =>
    candidate.id === task.id ? task : candidate,
  );
}

const initialLoading: BridgeLoadingState = {
  initial: true,
  proxy: false,
  models: false,
  browser: false,
  desktop: false,
  a2a: false,
  codeMemory: false,
  chat: false,
  login: false,
};

const initialErrors: BridgeErrorState = {
  initialization: null,
  proxy: null,
  models: null,
  browser: null,
  desktop: null,
  a2a: null,
  codeMemory: null,
  chat: null,
  login: null,
};

export function useBridgeState() {
  const [persisted] = useState(loadPersistedSession);
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [browserStatus, setBrowserStatus] = useState<BrowserStatus | null>(
    null,
  );
  const [browserFrame, setBrowserFrame] = useState<BrowserFrame | null>(null);
  const [desktopStatus, setDesktopStatus] = useState<DesktopStatus | null>(
    null,
  );
  const [a2aStatus, setA2aStatus] = useState<A2aStatus | null>(null);
  const [a2aTasks, setA2aTasks] = useState<A2aTask[]>([]);
  const [selectedA2aTask, setSelectedA2aTask] = useState<A2aTask | null>(null);
  const [codePolicyStatus, setCodePolicyStatus] =
    useState<CodePolicyStatus | null>(null);
  const [codeMemoryStatus, setCodeMemoryStatus] =
    useState<CodeMemoryStatus | null>(null);
  const [codeMemoryResults, setCodeMemoryResults] = useState<
    CodeMemorySearchResult[]
  >([]);
  const [codeMemoryWarnings, setCodeMemoryWarnings] = useState<string[]>([]);
  const [models, setModels] = useState<ProxyModel[]>([]);
  const [selectedModel, setSelectedModel] = useState(persisted.selectedModel);
  const [trace, setTrace] = useState<TraceEntry[]>(persisted.trace);
  const [chat, setChat] = useState<UiChatMessage[]>(persisted.chat);
  const [sending, setSending] = useState(false);
  const [loading, setLoading] = useState<BridgeLoadingState>(initialLoading);
  const [errors, setErrors] = useState<BridgeErrorState>(initialErrors);
  const [loginState, setLoginState] = useState<LoginState>({ phase: "idle" });
  const [loginVerification, setLoginVerification] =
    useState<LoginVerificationState>({ phase: "idle" });

  const sendingRef = useRef(false);
  const browserOperationRef = useRef(false);
  const desktopOperationRef = useRef(false);
  const a2aOperationRef = useRef(false);
  const a2aMutationGenerationRef = useRef(0);
  const codeMemoryOperationRef = useRef(false);
  const loginVerificationTokenRef = useRef(0);
  const pendingBrowserFrameRef = useRef<BrowserFrame | null>(null);
  const browserFrameRequestRef = useRef<number | null>(null);
  const lastBrowserSequenceRef = useRef(0);
  const a2aTasksRef = useRef(a2aTasks);

  useEffect(() => {
    a2aTasksRef.current = a2aTasks;
  }, [a2aTasks]);

  const setResourceLoading = useCallback(
    (resource: LoadingResource, value: boolean) => {
      setLoading((current) =>
        current[resource] === value
          ? current
          : { ...current, [resource]: value },
      );
    },
    [],
  );

  const setResourceError = useCallback(
    (resource: ErrorResource, value: string | null) => {
      setErrors((current) =>
        current[resource] === value
          ? current
          : { ...current, [resource]: value },
      );
    },
    [],
  );

  const appendTrace = useCallback(
    (message: string, kind: TraceEntry["kind"] = "info") => {
      const boundedMessage = truncateText(message, MAX_TRACE_MESSAGE_LENGTH);
      setTrace((current) => {
        const timestamp = new Date();
        const previous = current.at(-1);
        if (
          previous?.message === boundedMessage &&
          previous.kind === kind &&
          timestamp.getTime() - previous.timestamp.getTime() < 1_000
        ) {
          return current;
        }
        return boundedTrace([
          ...current,
          { id: nextId("trace"), timestamp, kind, message: boundedMessage },
        ]);
      });
    },
    [],
  );

  const applyModels = useCallback((nextModels: ProxyModel[]) => {
    setModels(nextModels);
    setSelectedModel((current) =>
      nextModels.some((model) => model.id === current)
        ? current
        : (nextModels[0]?.id ?? ""),
    );
  }, []);

  const refreshModels = useCallback(async () => {
    setResourceLoading("models", true);
    setResourceError("models", null);
    try {
      const nextModels = await fetchModelsShared();
      applyModels(nextModels);
      appendTrace(
        `Loaded ${nextModels.length} active model${nextModels.length === 1 ? "" : "s"}`,
        "success",
      );
    } catch (error) {
      const message = errorMessage(error);
      setResourceError("models", message);
      appendTrace(`Model refresh failed: ${message}`, "error");
    } finally {
      setResourceLoading("models", false);
    }
  }, [appendTrace, applyModels, setResourceError, setResourceLoading]);

  const handleChatChunk = useCallback(
    (chunk: ChatChunk) => {
      setChat((current) => {
        const index = current.findIndex(
          (message) => message.id === chunk.requestId,
        );
        const next =
          index < 0
            ? [
                ...current,
                {
                  id: chunk.requestId,
                  role: "assistant" as const,
                  content: chunk.delta,
                  streaming: !chunk.done,
                  error: chunk.error,
                },
              ]
            : current.map((message, messageIndex) =>
                messageIndex === index
                  ? {
                      ...message,
                      content: message.content + chunk.delta,
                      streaming: !chunk.done,
                      error: chunk.error,
                    }
                  : message,
              );
        return boundedChat(next);
      });
      if (chunk.done) {
        sendingRef.current = false;
        setSending(false);
        setResourceLoading("chat", false);
        setResourceError("chat", chunk.error ?? null);
        appendTrace(
          chunk.error
            ? `Model request failed: ${chunk.error}`
            : "Model response completed",
          chunk.error ? "error" : "success",
        );
      }
    },
    [appendTrace, setResourceError, setResourceLoading],
  );

  const handleSandboxValidation = useCallback(
    ({ requestId, validation }: SandboxValidationEvent) => {
      setChat((current) =>
        current.map((message) =>
          message.id === requestId ? { ...message, validation } : message,
        ),
      );
      if (validation.containsCode) {
        appendTrace(
          validation.valid
            ? `Tree-sitter validated ${validation.blocks.length} sandbox code block${validation.blocks.length === 1 ? "" : "s"}`
            : `Sandbox validation found ${validation.issues.length} issue${validation.issues.length === 1 ? "" : "s"}`,
          validation.valid ? "success" : "error",
        );
      }
    },
    [appendTrace],
  );

  const handleBrowserFrame = useCallback((frame: BrowserFrame) => {
    if (frame.sequence <= lastBrowserSequenceRef.current) {
      return;
    }
    pendingBrowserFrameRef.current = frame;
    if (browserFrameRequestRef.current !== null) {
      return;
    }
    browserFrameRequestRef.current = window.requestAnimationFrame(() => {
      browserFrameRequestRef.current = null;
      const pending = pendingBrowserFrameRef.current;
      if (!pending || pending.sequence <= lastBrowserSequenceRef.current) {
        return;
      }
      lastBrowserSequenceRef.current = pending.sequence;
      setBrowserFrame(pending);
      setBrowserStatus((current) => ({
        running: true,
        url: pending.url,
        viewportWidth: pending.width,
        viewportHeight: pending.height,
        health: current?.health === "degraded" ? "degraded" : "running",
        error: current?.health === "degraded" ? current.error : null,
      }));
    });
  }, []);

  useEffect(() => {
    const unsubscribe = subscribeBridgeEvents({
      proxyStatus: (status) => {
        setProxyStatus(status);
        setResourceError("proxy", status.error ?? null);
        appendTrace(
          status.running
            ? "CLIProxyAPI is listening on port 8317"
            : `CLIProxyAPI is unavailable: ${status.error ?? "unknown error"}`,
          status.running ? "success" : "error",
        );
        if (status.running) {
          void refreshModels();
        }
      },
      a2aStatus: (status) => {
        setA2aStatus(status);
        setResourceError("a2a", status.error ?? null);
        appendTrace(
          status.running
            ? `A2A server is listening on ${status.address}`
            : `A2A server failed: ${status.error ?? "unknown error"}`,
          status.running ? "success" : "error",
        );
      },
      chatChunk: handleChatChunk,
      sandboxValidation: handleSandboxValidation,
      browserFrame: handleBrowserFrame,
      codeMemoryStatus: (status) => {
        setCodeMemoryStatus(status);
        setResourceError("codeMemory", status.error ?? null);
      },
      bridgeError: (error) => {
        const message = errorMessage(error);
        setErrors((current) => ({ ...current, initialization: message }));
        appendTrace(`Tauri event bridge failed: ${message}`, "error");
      },
    });
    return () => {
      unsubscribe();
      if (browserFrameRequestRef.current !== null) {
        window.cancelAnimationFrame(browserFrameRequestRef.current);
        browserFrameRequestRef.current = null;
      }
    };
  }, [
    appendTrace,
    handleBrowserFrame,
    handleChatChunk,
    handleSandboxValidation,
    refreshModels,
    setResourceError,
  ]);

  const applyInitialSnapshot = useCallback(
    async (snapshot: InitialSnapshot, reportFailures: boolean) => {
      const applyFailure = (
        resource: ErrorResource,
        label: string,
        reason: unknown,
      ) => {
        const message = errorMessage(reason);
        setResourceError(resource, message);
        if (reportFailures) {
          appendTrace(`${label} failed: ${message}`, "error");
        }
      };

      if (snapshot.proxy.status === "fulfilled") {
        setProxyStatus(snapshot.proxy.value);
        setResourceError("proxy", snapshot.proxy.value.error ?? null);
        if (snapshot.proxy.value.running) {
          void refreshModels();
        }
      } else {
        applyFailure("proxy", "Proxy status", snapshot.proxy.reason);
      }
      if (snapshot.browser.status === "fulfilled") {
        setBrowserStatus(snapshot.browser.value);
        setResourceError("browser", snapshot.browser.value.error ?? null);
      } else {
        applyFailure("browser", "Browser status", snapshot.browser.reason);
      }
      if (snapshot.desktop.status === "fulfilled") {
        setDesktopStatus(snapshot.desktop.value);
        setResourceError("desktop", snapshot.desktop.value.error ?? null);
      } else {
        applyFailure("desktop", "Desktop status", snapshot.desktop.reason);
      }
      if (snapshot.a2a.status === "fulfilled") {
        setA2aStatus(snapshot.a2a.value);
        setResourceError("a2a", snapshot.a2a.value.error ?? null);
      } else {
        applyFailure("a2a", "A2A status", snapshot.a2a.reason);
      }
      if (snapshot.codePolicy.status === "fulfilled") {
        setCodePolicyStatus(snapshot.codePolicy.value);
      } else if (reportFailures) {
        appendTrace(
          `Code policy status failed: ${errorMessage(snapshot.codePolicy.reason)}`,
          "error",
        );
      }
      if (snapshot.codeMemory.status === "fulfilled") {
        setCodeMemoryStatus(snapshot.codeMemory.value);
        setResourceError("codeMemory", snapshot.codeMemory.value.error ?? null);
      } else {
        applyFailure(
          "codeMemory",
          "Code-memory status",
          snapshot.codeMemory.reason,
        );
      }
    },
    [appendTrace, refreshModels, setResourceError],
  );

  useEffect(() => {
    let disposed = false;
    void requestInitialSnapshot()
      .then(async (snapshot) => {
        if (!disposed) {
          await applyInitialSnapshot(snapshot, true);
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          const message = errorMessage(error);
          setErrors((current) => ({ ...current, initialization: message }));
          appendTrace(`Bridge initialization failed: ${message}`, "error");
        }
      })
      .finally(() => {
        if (!disposed) {
          setLoading((current) => ({ ...current, initial: false }));
        }
      });
    return () => {
      disposed = true;
      loginVerificationTokenRef.current += 1;
    };
  }, [appendTrace, applyInitialSnapshot]);

  const refreshA2aTasks = useCallback(
    async (silent = false) => {
      if (a2aOperationRef.current) {
        return;
      }
      const generation = a2aMutationGenerationRef.current;
      if (!silent) {
        setResourceLoading("a2a", true);
      }
      try {
        const tasks = await fetchTasksShared();
        if (generation !== a2aMutationGenerationRef.current) {
          return;
        }
        setA2aTasks(tasks);
        setSelectedA2aTask((current) =>
          current
            ? (tasks.find((task) => task.id === current.id) ?? null)
            : null,
        );
        setResourceError("a2a", null);
      } catch (error) {
        setResourceError("a2a", errorMessage(error));
      } finally {
        if (!silent) {
          setResourceLoading("a2a", false);
        }
      }
    },
    [setResourceError, setResourceLoading],
  );

  useEffect(() => {
    let disposed = false;
    let timeout: number | undefined;
    const poll = async () => {
      if (!document.hidden) {
        await refreshA2aTasks(true);
      }
      if (!disposed) {
        const working = a2aTasksRef.current.some(
          (task) => task.status.state === "TASK_STATE_WORKING",
        );
        timeout = window.setTimeout(() => void poll(), working ? 1_500 : 4_000);
      }
    };
    void poll();
    return () => {
      disposed = true;
      if (timeout !== undefined) {
        window.clearTimeout(timeout);
      }
    };
  }, [refreshA2aTasks]);

  useEffect(() => {
    const timeout = window.setTimeout(
      () => persistSession({ selectedModel, trace, chat }),
      300,
    );
    return () => window.clearTimeout(timeout);
  }, [chat, selectedModel, trace]);

  const refreshAll = useCallback(async () => {
    setLoading((current) => ({ ...current, initial: true }));
    setErrors((current) => ({ ...current, initialization: null }));
    try {
      const [proxy, browser, desktop, a2a, codePolicy, codeMemory] =
        await Promise.allSettled([
          backend.getProxyStatus(),
          backend.getBrowserStatus(),
          backend.getDesktopStatus(),
          backend.getA2aStatus(),
          backend.getCodePolicyStatus(),
          backend.getCodeMemoryStatus(),
        ]);
      await applyInitialSnapshot(
        { proxy, browser, desktop, a2a, codePolicy, codeMemory },
        true,
      );
      await refreshA2aTasks();
    } finally {
      setLoading((current) => ({ ...current, initial: false }));
    }
  }, [applyInitialSnapshot, refreshA2aTasks]);

  const ensureProxy = useCallback(async () => {
    if (loading.proxy) {
      return;
    }
    setResourceLoading("proxy", true);
    setResourceError("proxy", null);
    appendTrace("Starting CLIProxyAPI");
    try {
      const status = await backend.ensureProxy();
      setProxyStatus(status);
      appendTrace("CLIProxyAPI is ready", "success");
      await refreshModels();
    } catch (error) {
      const message = errorMessage(error);
      setResourceError("proxy", message);
      appendTrace(`CLIProxyAPI failed to start: ${message}`, "error");
    } finally {
      setResourceLoading("proxy", false);
    }
  }, [
    appendTrace,
    loading.proxy,
    refreshModels,
    setResourceError,
    setResourceLoading,
  ]);

  const runBrowserOperation = useCallback(
    async <T>(
      label: string,
      operation: () => Promise<T>,
    ): Promise<T | null> => {
      if (browserOperationRef.current) {
        appendTrace("Another browser operation is already running", "error");
        return null;
      }
      browserOperationRef.current = true;
      setResourceLoading("browser", true);
      setResourceError("browser", null);
      try {
        return await operation();
      } catch (error) {
        const message = errorMessage(error);
        setResourceError("browser", message);
        appendTrace(`${label} failed: ${message}`, "error");
        return null;
      } finally {
        browserOperationRef.current = false;
        setResourceLoading("browser", false);
      }
    },
    [appendTrace, setResourceError, setResourceLoading],
  );

  const refreshBrowserStatus = useCallback(async () => {
    const status = await runBrowserOperation("Browser status refresh", () =>
      backend.getBrowserStatus(),
    );
    if (status) {
      setBrowserStatus(status);
    }
  }, [runBrowserOperation]);

  const startBrowser = useCallback(async () => {
    appendTrace("Starting the isolated Chromium session");
    lastBrowserSequenceRef.current = 0;
    setBrowserFrame(null);
    const status = await runBrowserOperation("Agent browser startup", () =>
      backend.startBrowser(),
    );
    if (status) {
      setBrowserStatus(status);
      appendTrace("Agent browser is ready", "success");
    }
  }, [appendTrace, runBrowserOperation]);

  const stopBrowser = useCallback(async () => {
    const status = await runBrowserOperation("Agent browser shutdown", () =>
      backend.stopBrowser(),
    );
    if (status) {
      lastBrowserSequenceRef.current = 0;
      setBrowserFrame(null);
      setBrowserStatus(status);
      appendTrace("Agent browser stopped", "success");
    }
  }, [appendTrace, runBrowserOperation]);

  const navigateBrowser = useCallback(
    async (url: string) => {
      const address = url.trim();
      if (!address) {
        return;
      }
      appendTrace(`Navigating agent browser to ${truncateText(address, 512)}`);
      const completed = await runBrowserOperation(
        "Browser navigation",
        async () => {
          await backend.navigateBrowser(address);
          return true;
        },
      );
      if (completed) {
        const status = await backend.getBrowserStatus().catch(() => null);
        if (status) {
          setBrowserStatus(status);
        }
        appendTrace("Agent browser navigation completed", "success");
      }
    },
    [appendTrace, runBrowserOperation],
  );

  const clickBrowserAt = useCallback(
    async (x: number, y: number) => {
      if (!Number.isFinite(x) || !Number.isFinite(y)) {
        appendTrace("Browser click coordinates must be finite", "error");
        return;
      }
      const completed = await runBrowserOperation("Browser click", async () => {
        await backend.clickBrowserAt(x, y);
        return true;
      });
      if (completed) {
        appendTrace(`Browser click at ${Math.round(x)}, ${Math.round(y)}`);
      }
    },
    [appendTrace, runBrowserOperation],
  );

  const clickBrowserSelector = useCallback(
    async (selector: string) => {
      const target = selector.trim();
      if (!target) {
        appendTrace("Enter a browser selector before clicking", "error");
        return;
      }
      const completed = await runBrowserOperation(
        "Browser selector click",
        async () => {
          await backend.clickBrowserSelector(target);
          return true;
        },
      );
      if (completed) {
        appendTrace(
          `Clicked browser selector ${truncateText(target, 256)}`,
          "success",
        );
      }
    },
    [appendTrace, runBrowserOperation],
  );

  const typeInBrowser = useCallback(
    async (selector: string, text: string) => {
      const target = selector.trim();
      if (!target) {
        appendTrace("Enter a browser selector before typing", "error");
        return;
      }
      const completed = await runBrowserOperation(
        "Browser typing",
        async () => {
          await backend.typeInBrowser(target, text);
          return true;
        },
      );
      if (completed) {
        appendTrace(
          `Typed into browser selector ${truncateText(target, 256)}`,
          "success",
        );
      }
    },
    [appendTrace, runBrowserOperation],
  );

  const runDesktopOperation = useCallback(
    async <T>(
      label: string,
      operation: () => Promise<T>,
    ): Promise<T | null> => {
      if (desktopOperationRef.current) {
        appendTrace("Another desktop operation is already running", "error");
        return null;
      }
      desktopOperationRef.current = true;
      setResourceLoading("desktop", true);
      setResourceError("desktop", null);
      try {
        return await operation();
      } catch (error) {
        const message = errorMessage(error);
        setResourceError("desktop", message);
        appendTrace(`${label} failed: ${message}`, "error");
        return null;
      } finally {
        desktopOperationRef.current = false;
        setResourceLoading("desktop", false);
      }
    },
    [appendTrace, setResourceError, setResourceLoading],
  );

  const refreshDesktopStatus = useCallback(async () => {
    const status = await runDesktopOperation("Desktop status refresh", () =>
      backend.getDesktopStatus(),
    );
    if (status) {
      setDesktopStatus(status);
    }
  }, [runDesktopOperation]);

  const enableDesktopWorkMode = useCallback(async () => {
    const status = await runDesktopOperation("Desktop Work Mode enable", () =>
      backend.enableDesktopWorkMode(),
    );
    if (status) {
      setDesktopStatus(status);
      appendTrace("Desktop Work Mode enabled", "success");
    }
  }, [appendTrace, runDesktopOperation]);

  const disableDesktopWorkMode = useCallback(async () => {
    const status = await runDesktopOperation("Desktop Work Mode disable", () =>
      backend.disableDesktopWorkMode(),
    );
    if (status) {
      setDesktopStatus(status);
      appendTrace("Desktop Work Mode disabled", "success");
    }
  }, [appendTrace, runDesktopOperation]);

  const clickDesktopAt = useCallback(
    async (x: number, y: number) => {
      if (!Number.isFinite(x) || !Number.isFinite(y)) {
        appendTrace("Desktop coordinates must be finite", "error");
        return;
      }
      const roundedX = Math.round(x);
      const roundedY = Math.round(y);
      const completed = await runDesktopOperation("Desktop click", async () => {
        await backend.clickDesktopAt(roundedX, roundedY);
        return true;
      });
      if (completed) {
        appendTrace(`Desktop click at ${roundedX}, ${roundedY}`);
      }
    },
    [appendTrace, runDesktopOperation],
  );

  const typeOnDesktop = useCallback(
    async (text: string) => {
      if (!text) {
        return;
      }
      const completed = await runDesktopOperation(
        "Desktop typing",
        async () => {
          await backend.typeOnDesktop(text);
          return true;
        },
      );
      if (completed) {
        appendTrace(
          `Typed ${text.length} character${text.length === 1 ? "" : "s"} on desktop`,
        );
      }
    },
    [appendTrace, runDesktopOperation],
  );

  const runA2aOperation = useCallback(
    async <T>(
      label: string,
      operation: () => Promise<T>,
    ): Promise<T | null> => {
      if (a2aOperationRef.current) {
        appendTrace("Another A2A operation is already running", "error");
        return null;
      }
      a2aOperationRef.current = true;
      setResourceLoading("a2a", true);
      setResourceError("a2a", null);
      try {
        return await operation();
      } catch (error) {
        const message = errorMessage(error);
        setResourceError("a2a", message);
        appendTrace(`${label} failed: ${message}`, "error");
        return null;
      } finally {
        a2aOperationRef.current = false;
        setResourceLoading("a2a", false);
      }
    },
    [appendTrace, setResourceError, setResourceLoading],
  );

  const getA2aTask = useCallback(
    async (id: string) => {
      const task = await runA2aOperation("A2A task refresh", () =>
        backend.getA2aTask(id),
      );
      if (task) {
        setA2aTasks((current) => replaceTask(current, task));
        setSelectedA2aTask(task);
      }
      return task;
    },
    [runA2aOperation],
  );

  const selectA2aTask = useCallback(
    async (id: string | null) => {
      if (!id) {
        setSelectedA2aTask(null);
        return null;
      }
      const cached = a2aTasksRef.current.find((task) => task.id === id);
      if (cached) {
        setSelectedA2aTask(cached);
      }
      return getA2aTask(id);
    },
    [getA2aTask],
  );

  const delegateA2aTask = useCallback(
    async (request: DelegateA2aTaskRequest) => {
      const prompt = request.prompt.trim();
      if (!prompt) {
        appendTrace("Enter an A2A task before delegating", "error");
        return null;
      }
      a2aMutationGenerationRef.current += 1;
      const task = await runA2aOperation("A2A delegation", () =>
        backend.delegateA2aTask({
          ...request,
          prompt,
          model: request.model || selectedModel || null,
        }),
      );
      if (task) {
        setA2aTasks((current) => replaceTask(current, task));
        setSelectedA2aTask(task);
        appendTrace(`Delegated A2A task ${task.id}`, "success");
      }
      return task;
    },
    [appendTrace, runA2aOperation, selectedModel],
  );

  const cancelA2aTask = useCallback(
    async (id: string) => {
      a2aMutationGenerationRef.current += 1;
      const task = await runA2aOperation("A2A cancellation", () =>
        backend.cancelA2aTask(id),
      );
      if (task) {
        setA2aTasks((current) => replaceTask(current, task));
        setSelectedA2aTask((current) => (current?.id === id ? task : current));
        appendTrace(`Canceled A2A task ${id}`, "success");
      }
      return task;
    },
    [appendTrace, runA2aOperation],
  );

  const runCodeMemoryOperation = useCallback(
    async <T>(
      label: string,
      operation: () => Promise<T>,
    ): Promise<T | null> => {
      if (codeMemoryOperationRef.current) {
        appendTrace(
          "Another code-memory operation is already running",
          "error",
        );
        return null;
      }
      codeMemoryOperationRef.current = true;
      setResourceLoading("codeMemory", true);
      setResourceError("codeMemory", null);
      try {
        return await operation();
      } catch (error) {
        const message = errorMessage(error);
        setResourceError("codeMemory", message);
        appendTrace(`${label} failed: ${message}`, "error");
        return null;
      } finally {
        codeMemoryOperationRef.current = false;
        setResourceLoading("codeMemory", false);
      }
    },
    [appendTrace, setResourceError, setResourceLoading],
  );

  const refreshCodeMemoryStatus = useCallback(async () => {
    const status = await runCodeMemoryOperation(
      "Code-memory status refresh",
      () => backend.getCodeMemoryStatus(),
    );
    if (status) {
      setCodeMemoryStatus(status);
    }
  }, [runCodeMemoryOperation]);

  const indexCodeMemory = useCallback(
    async (root: string) => {
      const selectedRoot = root.trim();
      if (!selectedRoot) {
        appendTrace("Choose a source directory to index", "error");
        return false;
      }
      setCodeMemoryStatus((current) =>
        current ? { ...current, indexing: true, error: null } : current,
      );
      const result = await runCodeMemoryOperation("Code-memory indexing", () =>
        backend.indexCodeMemory(selectedRoot),
      );
      if (!result) {
        setCodeMemoryStatus((current) =>
          current ? { ...current, indexing: false } : current,
        );
        return false;
      }
      setCodeMemoryStatus(result.status);
      setCodeMemoryWarnings(result.warnings.slice(0, 256));
      setCodeMemoryResults([]);
      appendTrace(
        `Indexed ${result.status.statistics.indexedFiles} source file${result.status.statistics.indexedFiles === 1 ? "" : "s"} into ${result.status.statistics.chunks} structural chunks`,
        "success",
      );
      return true;
    },
    [appendTrace, runCodeMemoryOperation],
  );

  const searchCodeMemory = useCallback(
    async (
      query: string,
      options: { maxResults?: number; graphWeight?: number } = {},
    ) => {
      const searchQuery = query.trim();
      if (!searchQuery) {
        setCodeMemoryResults([]);
        return [];
      }
      const maxResults = Math.min(
        100,
        Math.max(1, Math.round(options.maxResults ?? 12)),
      );
      const graphWeight = Math.min(2, Math.max(0, options.graphWeight ?? 0.25));
      const results = await runCodeMemoryOperation("Code-memory search", () =>
        backend.searchCodeMemory({
          query: searchQuery,
          maxResults,
          graphWeight,
        }),
      );
      if (!results) {
        return [];
      }
      setCodeMemoryResults(results);
      appendTrace(
        `Code memory returned ${results.length} result${results.length === 1 ? "" : "s"}`,
        "success",
      );
      return results;
    },
    [appendTrace, runCodeMemoryOperation],
  );

  const clearCodeMemory = useCallback(async () => {
    const status = await runCodeMemoryOperation("Code-memory cleanup", () =>
      backend.clearCodeMemory(),
    );
    if (status) {
      setCodeMemoryStatus(status);
      setCodeMemoryResults([]);
      setCodeMemoryWarnings([]);
      appendTrace("Code memory cleared", "success");
    }
  }, [appendTrace, runCodeMemoryOperation]);

  const clearCodeMemoryResults = useCallback(
    () => setCodeMemoryResults([]),
    [],
  );

  const sendPrompt = useCallback(
    async (prompt: string) => {
      const content = prompt.trim();
      if (!content || sendingRef.current) {
        return;
      }
      if (!selectedModel) {
        appendTrace("Select an active model before sending", "error");
        return;
      }
      if (utf8Length(content) > MAX_CHAT_REQUEST_LENGTH) {
        appendTrace(
          `Prompt exceeds the ${MAX_CHAT_REQUEST_LENGTH}-byte request limit`,
          "error",
        );
        return;
      }

      const userMessage: UiChatMessage = {
        id: nextId("user"),
        role: "user",
        content,
      };
      const history = requestHistory(
        chat,
        MAX_CHAT_REQUEST_LENGTH - utf8Length(content),
      );
      setChat((current) => boundedChat([...current, userMessage]));
      sendingRef.current = true;
      setSending(true);
      setResourceLoading("chat", true);
      setResourceError("chat", null);
      appendTrace(`Sending prompt to ${selectedModel}`);
      try {
        const requestId = await backend.startChat({
          model: selectedModel,
          messages: [...history, { role: "user", content }],
        });
        setChat((current) =>
          current.some((message) => message.id === requestId)
            ? current
            : boundedChat([
                ...current,
                {
                  id: requestId,
                  role: "assistant",
                  content: "",
                  streaming: true,
                },
              ]),
        );
      } catch (error) {
        sendingRef.current = false;
        setSending(false);
        setResourceLoading("chat", false);
        const message = errorMessage(error);
        setResourceError("chat", message);
        setChat((current) =>
          boundedChat([
            ...current,
            {
              id: nextId("assistant-error"),
              role: "assistant",
              content: "",
              error: message,
            },
          ]),
        );
        appendTrace(`Prompt failed: ${message}`, "error");
      }
    },
    [appendTrace, chat, selectedModel, setResourceError, setResourceLoading],
  );

  const retryLastPrompt = useCallback(async () => {
    const previous = [...chat]
      .reverse()
      .find((message) => message.role === "user");
    if (!previous) {
      appendTrace("There is no previous prompt to retry", "error");
      return;
    }
    await sendPrompt(previous.content);
  }, [appendTrace, chat, sendPrompt]);

  const clearTrace = useCallback(() => setTrace([]), []);
  const clearChat = useCallback(() => {
    if (sendingRef.current) {
      appendTrace("Wait for the active response before clearing chat", "error");
      return;
    }
    setChat([]);
    setResourceError("chat", null);
  }, [appendTrace, setResourceError]);

  const verifyLogin = useCallback(async () => {
    const token = loginVerificationTokenRef.current + 1;
    loginVerificationTokenRef.current = token;
    setResourceLoading("login", true);
    setResourceError("login", null);
    for (let attempt = 1; attempt <= LOGIN_POLL_ATTEMPTS; attempt += 1) {
      if (token !== loginVerificationTokenRef.current) {
        return false;
      }
      setLoginVerification({ phase: "checking", attempt });
      if (attempt > 1) {
        await new Promise<void>((resolve) => {
          window.setTimeout(resolve, LOGIN_POLL_INTERVAL_MS);
        });
      }
      try {
        const nextModels = await fetchModelsShared();
        if (token !== loginVerificationTokenRef.current) {
          return false;
        }
        if (nextModels.length > 0) {
          applyModels(nextModels);
          setLoginVerification({
            phase: "verified",
            modelCount: nextModels.length,
            verifiedAt: new Date().toISOString(),
          });
          setResourceLoading("login", false);
          appendTrace(
            "ChatGPT login verified through the active model registry",
            "success",
          );
          return true;
        }
      } catch {
        // OAuth completion is asynchronous; keep polling within the fixed window.
      }
    }
    const message =
      "Login has not been verified yet. Finish OAuth, then verify again.";
    setLoginVerification({ phase: "pending", error: message });
    setResourceError("login", message);
    setResourceLoading("login", false);
    appendTrace(message, "error");
    return false;
  }, [appendTrace, applyModels, setResourceError, setResourceLoading]);

  const launchLogin = useCallback(async () => {
    setLoginState({ phase: "launching" });
    setResourceLoading("login", true);
    setResourceError("login", null);
    appendTrace("Opening CLIProxyAPI Codex login");
    try {
      const launch = await backend.startLogin();
      setLoginState({ phase: "launched", launch });
      setResourceLoading("login", false);
      appendTrace("Browser login was launched", "success");
      void verifyLogin();
    } catch (error) {
      const message = errorMessage(error);
      setLoginState({ phase: "error", error: message });
      setResourceError("login", message);
      setResourceLoading("login", false);
      appendTrace(`Browser login failed: ${message}`, "error");
    }
  }, [appendTrace, setResourceError, setResourceLoading, verifyLogin]);

  const resetLogin = useCallback(() => {
    loginVerificationTokenRef.current += 1;
    setLoginState({ phase: "idle" });
    setLoginVerification({ phase: "idle" });
    setResourceLoading("login", false);
    setResourceError("login", null);
  }, [setResourceError, setResourceLoading]);

  return {
    proxyStatus,
    browserStatus,
    browserFrame,
    desktopStatus,
    a2aStatus,
    a2aTasks,
    selectedA2aTask,
    codePolicyStatus,
    codeMemoryStatus,
    codeMemoryResults,
    codeMemoryWarnings,
    models,
    selectedModel,
    setSelectedModel,
    trace,
    chat,
    sending,
    browserBusy: loading.browser,
    desktopBusy: loading.desktop,
    a2aBusy: loading.a2a,
    codeMemoryBusy: loading.codeMemory,
    loginState,
    setLoginState,
    loginVerification,
    loading,
    errors,
    refreshAll,
    refreshModels,
    ensureProxy,
    refreshBrowserStatus,
    startBrowser,
    stopBrowser,
    navigateBrowser,
    clickBrowserAt,
    clickBrowserSelector,
    typeInBrowser,
    refreshDesktopStatus,
    enableDesktopWorkMode,
    disableDesktopWorkMode,
    clickDesktopAt,
    typeOnDesktop,
    refreshA2aTasks,
    selectA2aTask,
    getA2aTask,
    delegateA2aTask,
    cancelA2aTask,
    refreshCodeMemoryStatus,
    indexCodeMemory,
    searchCodeMemory,
    clearCodeMemory,
    clearCodeMemoryResults,
    sendPrompt,
    retryLastPrompt,
    clearTrace,
    clearChat,
    launchLogin,
    verifyLogin,
    resetLogin,
  };
}
