import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { backend } from "./backend";
import { publishBrowserFrame } from "./browserFrameStore";
import type { BridgeHookContext } from "./bridgeHookContext";
import type {
  A2aStatus,
  BrowserStatus,
  CodeMemoryStatus,
  CodePolicyStatus,
  DesktopStatus,
  ProxyModel,
  ProxyStatus,
} from "./types";
import { useA2aBridgeState } from "./useA2aBridgeState";
import { useBrowserBridgeState } from "./useBrowserBridgeState";
import { useCodeMemoryBridgeState } from "./useCodeMemoryBridgeState";
import type { TraceEntry } from "./workbenchTypes";

const MAX_TRACE_ENTRIES = 200;
const MAX_TRACE_MESSAGE_LENGTH = 4_096;
export type BridgeLoadingState = {
  initial: boolean;
  proxy: boolean;
  models: boolean;
  browser: boolean;
  desktop: boolean;
  a2a: boolean;
  codeMemory: boolean;
};

export type BridgeErrorState = {
  initialization: string | null;
  proxy: string | null;
  models: string | null;
  browser: string | null;
  desktop: string | null;
  a2a: string | null;
  codeMemory: string | null;
};

type LoadingResource = Exclude<keyof BridgeLoadingState, "initial">;
type ErrorResource = Exclude<keyof BridgeErrorState, "initialization">;

type BridgeEventSubscriber = {
  proxyStatus: (status: ProxyStatus) => void;
  a2aStatus: (status: A2aStatus) => void;
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

const eventSubscribers = new Set<BridgeEventSubscriber>();
let eventBridgePromise: Promise<void> | null = null;
let initialSnapshotPromise: Promise<InitialSnapshot> | null = null;
let modelRequestPromise: Promise<ProxyModel[]> | null = null;
let localId = 0;

function nextId(prefix: string): string {
  localId += 1;
  return `${prefix}-${Date.now()}-${localId}`;
}

function errorMessage(error: unknown): string {
  let message: string;
  if (error instanceof Error) {
    message = error.message;
  } else if (typeof error === "string") {
    message = error;
  } else {
    try {
      message = JSON.stringify(error) || String(error);
    } catch {
      message = String(error);
    }
  }

  if (
    message.includes("reading 'invoke'") ||
    message.includes('reading "invoke"') ||
    message.includes("transformCallback")
  ) {
    return "Native Bridge services are unavailable in this web preview. Open the installed desktop app to connect local tools.";
  }

  return message;
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

function boundedTrace(entries: TraceEntry[]): TraceEntry[] {
  return entries.slice(-MAX_TRACE_ENTRIES);
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
      backend.onBrowserFrame(publishBrowserFrame),
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

const initialLoading: BridgeLoadingState = {
  initial: true,
  proxy: false,
  models: false,
  browser: false,
  desktop: false,
  a2a: false,
  codeMemory: false,
};

const initialErrors: BridgeErrorState = {
  initialization: null,
  proxy: null,
  models: null,
  browser: null,
  desktop: null,
  a2a: null,
  codeMemory: null,
};

export function useBridgeState() {
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const probedModelRef = useRef<string | null>(null);
  const [desktopStatus, setDesktopStatus] = useState<DesktopStatus | null>(
    null,
  );
  const [codePolicyStatus, setCodePolicyStatus] =
    useState<CodePolicyStatus | null>(null);
  const [models, setModels] = useState<ProxyModel[]>([]);
  const [selectedModel, setSelectedModel] = useState("");
  const [trace, setTrace] = useState<TraceEntry[]>([]);
  const [loading, setLoading] = useState<BridgeLoadingState>(initialLoading);
  const [errors, setErrors] = useState<BridgeErrorState>(initialErrors);

  const desktopOperationRef = useRef(false);

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
    const probedModel = probedModelRef.current;
    setSelectedModel(
      nextModels.some((model) => model.id === probedModel)
        ? (probedModel ?? "")
        : "",
    );
  }, []);

  const selectModel = useCallback(
    (model: string) => {
      if (model !== probedModelRef.current) {
        const message = `${model} is unavailable until it passes the Responses and tool-call conformance probe.`;
        setResourceError("models", message);
        appendTrace(message, "error");
        return;
      }
      setResourceError("models", null);
      setSelectedModel(model);
    },
    [appendTrace, setResourceError],
  );

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

  const browserContext = useMemo<BridgeHookContext>(
    () => ({
      appendTrace,
      setBusy: (busy) => setResourceLoading("browser", busy),
      setError: (error) => setResourceError("browser", error),
    }),
    [appendTrace, setResourceError, setResourceLoading],
  );
  const a2aContext = useMemo<BridgeHookContext>(
    () => ({
      appendTrace,
      setBusy: (busy) => setResourceLoading("a2a", busy),
      setError: (error) => setResourceError("a2a", error),
    }),
    [appendTrace, setResourceError, setResourceLoading],
  );
  const codeMemoryContext = useMemo<BridgeHookContext>(
    () => ({
      appendTrace,
      setBusy: (busy) => setResourceLoading("codeMemory", busy),
      setError: (error) => setResourceError("codeMemory", error),
    }),
    [appendTrace, setResourceError, setResourceLoading],
  );
  const browser = useBrowserBridgeState(browserContext);
  const a2a = useA2aBridgeState(a2aContext, selectedModel);
  const codeMemory = useCodeMemoryBridgeState(codeMemoryContext);

  useEffect(() => {
    const unsubscribe = subscribeBridgeEvents({
      proxyStatus: (status) => {
        probedModelRef.current = status.probedModel ?? null;
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
        a2a.applyStatus(status);
        appendTrace(
          status.running
            ? `A2A server is listening on ${status.address}`
            : `A2A server failed: ${status.error ?? "unknown error"}`,
          status.running ? "success" : "error",
        );
      },
      codeMemoryStatus: (status) => {
        codeMemory.applyStatus(status);
      },
      bridgeError: (error) => {
        const message = errorMessage(error);
        setErrors((current) => ({ ...current, initialization: message }));
        appendTrace(`Tauri event bridge failed: ${message}`, "error");
      },
    });
    return unsubscribe;
  }, [
    appendTrace,
    a2a.applyStatus,
    codeMemory.applyStatus,
    refreshModels,
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
        probedModelRef.current = snapshot.proxy.value.probedModel ?? null;
        setProxyStatus(snapshot.proxy.value);
        setResourceError("proxy", snapshot.proxy.value.error ?? null);
        if (snapshot.proxy.value.running) {
          void refreshModels();
        }
      } else {
        applyFailure("proxy", "Proxy status", snapshot.proxy.reason);
      }
      if (snapshot.browser.status === "fulfilled") {
        browser.applyStatus(snapshot.browser.value);
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
        a2a.applyStatus(snapshot.a2a.value);
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
        codeMemory.applyStatus(snapshot.codeMemory.value);
      } else {
        applyFailure(
          "codeMemory",
          "Code-memory status",
          snapshot.codeMemory.reason,
        );
      }
    },
    [
      a2a.applyStatus,
      appendTrace,
      browser.applyStatus,
      codeMemory.applyStatus,
      refreshModels,
      setResourceError,
    ],
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
    };
  }, [appendTrace, applyInitialSnapshot]);

  const refreshAll = useCallback(async () => {
    setLoading((current) => ({ ...current, initial: true }));
    setErrors((current) => ({ ...current, initialization: null }));
    try {
      const [proxy, browser, desktop, a2aResult, codePolicy, codeMemory] =
        await Promise.allSettled([
          backend.getProxyStatus(),
          backend.getBrowserStatus(),
          backend.getDesktopStatus(),
          backend.getA2aStatus(),
          backend.getCodePolicyStatus(),
          backend.getCodeMemoryStatus(),
        ]);
      await applyInitialSnapshot(
        { proxy, browser, desktop, a2a: a2aResult, codePolicy, codeMemory },
        true,
      );
      await a2a.refreshTasks();
    } finally {
      setLoading((current) => ({ ...current, initial: false }));
    }
  }, [a2a.refreshTasks, applyInitialSnapshot]);

  const configureProxy = useCallback(async (baseUrl: string, apiKey: string) => {
    if (loading.proxy) {
      return;
    }
    setResourceLoading("proxy", true);
    setResourceError("proxy", null);
    appendTrace("Connecting to the loopback model proxy");
    try {
      const status = await backend.configureProxy(baseUrl, apiKey);
      probedModelRef.current = status.probedModel ?? null;
      setProxyStatus(status);
      appendTrace("Loopback model proxy connected", "success");
      await refreshModels();
    } catch (error) {
      const message = errorMessage(error);
      setResourceError("proxy", message);
      appendTrace(`Model proxy connection failed: ${message}`, "error");
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

  const clearTrace = useCallback(() => setTrace([]), []);
  const resetLocalSession = useCallback(() => {
    setSelectedModel("");
    setTrace([]);
    browser.reset();
    a2a.reset();
    codeMemory.reset();
    setErrors(initialErrors);
  }, [a2a.reset, browser.reset, codeMemory.reset]);

  return {
    proxyStatus,
    browserStatus: browser.status,
    desktopStatus,
    a2aStatus: a2a.status,
    a2aTasks: a2a.tasks,
    selectedA2aTask: a2a.selectedTask,
    codePolicyStatus,
    codeMemoryStatus: codeMemory.status,
    codeMemoryResults: codeMemory.results,
    codeMemoryWarnings: codeMemory.warnings,
    models,
    selectedModel,
    setSelectedModel: selectModel,
    trace,
    browserBusy: loading.browser,
    desktopBusy: loading.desktop,
    a2aBusy: loading.a2a,
    codeMemoryBusy: loading.codeMemory,
    loading,
    errors,
    refreshAll,
    refreshModels,
    configureProxy,
    refreshBrowserStatus: browser.refreshStatus,
    startBrowser: browser.start,
    stopBrowser: browser.stop,
    navigateBrowser: browser.navigate,
    clickBrowserAt: browser.clickAt,
    clickBrowserSelector: browser.clickSelector,
    typeInBrowser: browser.type,
    refreshDesktopStatus,
    enableDesktopWorkMode,
    disableDesktopWorkMode,
    clickDesktopAt,
    typeOnDesktop,
    refreshA2aTasks: a2a.refreshTasks,
    selectA2aTask: a2a.selectTask,
    getA2aTask: a2a.getTask,
    delegateA2aTask: a2a.delegateTask,
    cancelA2aTask: a2a.cancelTask,
    configureA2aServer: a2a.configureServer,
    provisionA2aToken: a2a.provisionToken,
    deleteA2aToken: a2a.deleteToken,
    refreshCodeMemoryStatus: codeMemory.refreshStatus,
    indexCodeMemory: codeMemory.index,
    searchCodeMemory: codeMemory.search,
    clearCodeMemory: codeMemory.clear,
    clearCodeMemoryResults: codeMemory.clearResults,
    clearTrace,
    resetLocalSession,
  };
}
