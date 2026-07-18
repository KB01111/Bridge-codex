import { useCallback, useEffect, useMemo, useReducer, useRef } from "react";

import type { RequestId } from "../../app-server-protocol/schema/typescript/RequestId";
import type { ServerRequest } from "../../app-server-protocol/schema/typescript/ServerRequest";
import type { DynamicToolCallResponse } from "../../app-server-protocol/schema/typescript/v2/DynamicToolCallResponse";
import { agentRuntime, type BridgeAgentEvent } from "./agentRuntime";
import { backend } from "./backend";
import {
  agentRuntimeReducer,
  initialAgentRuntimeState,
  runtimeMessagesFromThread,
  runtimeThreadSummaries,
} from "./agentRuntimeReducer";
import { createConversationArchive } from "./localData";

function errorMessage(error: unknown): string {
  return error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "The agent runtime request failed.";
}

function supportedServerRequest(request: ServerRequest): boolean {
  switch (request.method) {
    case "item/commandExecution/requestApproval":
    case "item/fileChange/requestApproval":
    case "item/permissions/requestApproval":
    case "item/tool/requestUserInput":
    case "mcpServer/elicitation/request":
    case "item/tool/call":
      return true;
    case "account/chatgptAuthTokens/refresh":
    case "attestation/generate":
    case "applyPatchApproval":
    case "execCommandApproval":
      return false;
  }
}

function clientMessageId(): string {
  return typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `bridge-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

type DynamicToolRequest = Extract<
  ServerRequest,
  { method: "item/tool/call" }
>;

/** Executes a dynamic tool only after the user has approved its pending request. */
export async function executeUserApprovedDynamicTool(
  request: DynamicToolRequest,
): Promise<DynamicToolCallResponse> {
  if (request.params.tool.startsWith("browser_")) {
    await backend.grantAgentBrowserConsent(request.params.threadId);
  }
  const result = await agentRuntime.executeDynamicTool(request.params);
  await agentRuntime.resolveRequest(request.id, result);
  return result;
}

export async function hydratePendingServerRequests(
  receive: (request: ServerRequest) => void,
): Promise<void> {
  const pendingRequests = await agentRuntime.listPendingRequests();
  for (const pending of pendingRequests) {
    receive(pending.payload);
  }
}

export function useAgentRuntime(selectedModel: string) {
  const [state, dispatch] = useReducer(
    agentRuntimeReducer,
    initialAgentRuntimeState,
  );
  const stateRef = useRef(state);
  stateRef.current = state;

  const refreshThreads = useCallback(async (readActive = false) => {
    const response = await agentRuntime.request("thread/list", {
      limit: 100,
      archived: false,
    });
    dispatch({ type: "threads_loaded", threads: response.data });
    const activeId = stateRef.current.activeThread?.id;
    if (readActive && activeId) {
      const read = await agentRuntime.request("thread/read", {
        threadId: activeId,
        includeTurns: true,
      });
      dispatch({ type: "thread_loaded", thread: read.thread });
    }
    return response.data;
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlistenEvent: (() => void) | undefined;
    let unlistenStatus: (() => void) | undefined;
    const rejectedUnsupportedRequests = new Set<string>();

    function receiveServerRequest(request: ServerRequest) {
      dispatch({ type: "request_received", request });
      if (supportedServerRequest(request)) {
        return;
      }
      const { id, method } = request;
      const requestKey = `${typeof id}:${String(id)}`;
      if (rejectedUnsupportedRequests.has(requestKey)) {
        return;
      }
      rejectedUnsupportedRequests.add(requestKey);
      dispatch({ type: "request_resolving", requestId: id });
      void agentRuntime
        .rejectRequest(
          id,
          `Bridge Codex denies unsupported server request ${method}.`,
        )
        .then(() => dispatch({ type: "request_resolved", requestId: id }))
        .catch((error: unknown) =>
          dispatch({
            type: "request_failed",
            requestId: id,
            error: errorMessage(error),
          }),
        );
    }

    function handleEvent(event: BridgeAgentEvent) {
      if (event.kind === "serverNotification") {
        dispatch({ type: "notification", notification: event.payload });
        return;
      }
      if (event.kind === "serverRequest") {
        receiveServerRequest(event.payload);
        return;
      }
      if (event.kind === "lagged") {
        dispatch({ type: "lagged", skipped: event.skipped });
        void Promise.all([
          refreshThreads(true),
          hydratePendingServerRequests(receiveServerRequest),
        ]).catch((error: unknown) =>
          dispatch({ type: "failed", error: errorMessage(error) }),
        );
        return;
      }
      dispatch({ type: "disconnected", message: event.message });
    }

    async function initialize() {
      try {
        [unlistenEvent, unlistenStatus] = await Promise.all([
          agentRuntime.onEvent(handleEvent),
          agentRuntime.onStatus((status) => dispatch({ type: "status", status })),
        ]);
        const status = await agentRuntime.ensureStarted();
        if (disposed) {
          return;
        }
        dispatch({ type: "status", status });
        const [threads] = await Promise.all([
          refreshThreads(),
          hydratePendingServerRequests(receiveServerRequest),
        ]);
        const firstThread = threads[0];
        if (!stateRef.current.activeThread && firstThread) {
          const resumed = await agentRuntime.request("thread/resume", {
            threadId: firstThread.id,
          });
          dispatch({ type: "thread_loaded", thread: resumed.thread });
        }
      } catch (error) {
        if (!disposed) {
          dispatch({ type: "failed", error: errorMessage(error) });
        }
      }
    }

    void initialize();
    return () => {
      disposed = true;
      unlistenEvent?.();
      unlistenStatus?.();
    };
  }, [refreshThreads]);

  const startThread = useCallback(
    async (repositoryRoot: string) => {
      dispatch({ type: "loading", loading: true });
      try {
        const response = await agentRuntime.request("thread/start", {
          cwd: repositoryRoot.trim() || null,
          model: selectedModel || null,
        });
        dispatch({ type: "thread_loaded", thread: response.thread });
        dispatch({ type: "loading", loading: false });
        await refreshThreads();
        return response.thread;
      } catch (error) {
        dispatch({ type: "failed", error: errorMessage(error) });
        return null;
      }
    },
    [refreshThreads, selectedModel],
  );

  const resumeThread = useCallback(async (threadId: string) => {
    dispatch({ type: "loading", loading: true });
    try {
      const response = await agentRuntime.request("thread/resume", { threadId });
      dispatch({ type: "thread_loaded", thread: response.thread });
      dispatch({ type: "loading", loading: false });
    } catch (error) {
      dispatch({ type: "failed", error: errorMessage(error) });
    }
  }, []);

  const forkThread = useCallback(async () => {
    const threadId = stateRef.current.activeThread?.id;
    if (!threadId || stateRef.current.sending) {
      return;
    }
    dispatch({ type: "loading", loading: true });
    try {
      const response = await agentRuntime.request("thread/fork", {
        threadId,
        model: selectedModel || null,
      });
      dispatch({ type: "thread_loaded", thread: response.thread });
      dispatch({ type: "loading", loading: false });
      await refreshThreads();
    } catch (error) {
      dispatch({ type: "failed", error: errorMessage(error) });
    }
  }, [refreshThreads, selectedModel]);

  const archiveThread = useCallback(async () => {
    const threadId = stateRef.current.activeThread?.id;
    if (!threadId || stateRef.current.sending) {
      return;
    }
    dispatch({ type: "loading", loading: true });
    try {
      await backend.revokeAgentBrowserConsent(threadId);
      await agentRuntime.request("thread/archive", { threadId });
      const threads = await refreshThreads();
      const next = threads.find(
        (thread) => thread.id !== threadId,
      );
      if (next) {
        await resumeThread(next.id);
      } else {
        dispatch({ type: "session_reset" });
      }
    } catch (error) {
      dispatch({ type: "failed", error: errorMessage(error) });
    }
  }, [refreshThreads, resumeThread]);

  const nameThread = useCallback(async (name: string) => {
    const threadId = stateRef.current.activeThread?.id;
    if (!threadId || !name.trim()) {
      return;
    }
    try {
      await agentRuntime.request("thread/name/set", {
        threadId,
        name: name.trim(),
      });
      await refreshThreads(true);
    } catch (error) {
      dispatch({ type: "failed", error: errorMessage(error) });
    }
  }, [refreshThreads]);

  const sendPrompt = useCallback(
    async (prompt: string, repositoryRoot: string) => {
      const content = prompt.trim();
      if (!content) {
        return;
      }
      let thread = stateRef.current.activeThread;
      if (!thread) {
        thread = await startThread(repositoryRoot);
      }
      if (!thread) {
        return;
      }

      const id = clientMessageId();
      dispatch({ type: "local_user_message", id, content });
      const input = [{ type: "text" as const, text: content, text_elements: [] }];
      try {
        const activeTurnId = stateRef.current.activeTurnId;
        if (activeTurnId) {
          await agentRuntime.request("turn/steer", {
            threadId: thread.id,
            expectedTurnId: activeTurnId,
            clientUserMessageId: id,
            input,
          });
        } else {
          const response = await agentRuntime.request("turn/start", {
            threadId: thread.id,
            clientUserMessageId: id,
            input,
            cwd: repositoryRoot.trim() || null,
            model: selectedModel || null,
          });
          if (response.turn.status === "inProgress") {
            dispatch({
              type: "notification",
              notification: {
                method: "turn/started",
                params: { threadId: thread.id, turn: response.turn },
              },
            });
          }
        }
      } catch (error) {
        dispatch({ type: "failed", error: errorMessage(error) });
      }
    },
    [selectedModel, startThread],
  );

  const retryLastPrompt = useCallback(
    async (repositoryRoot: string) => {
      const previous = [...stateRef.current.messages]
        .reverse()
        .find((message) => message.role === "user");
      if (previous) {
        await sendPrompt(previous.content, repositoryRoot);
      }
    },
    [sendPrompt],
  );

  const interruptTurn = useCallback(async () => {
    const threadId = stateRef.current.activeThread?.id;
    const turnId = stateRef.current.activeTurnId;
    if (!threadId || !turnId) {
      return;
    }
    try {
      await agentRuntime.request("turn/interrupt", { threadId, turnId });
    } catch (error) {
      dispatch({ type: "failed", error: errorMessage(error) });
    }
  }, []);

  const resolveRequest = useCallback(
    async (requestId: RequestId, result: unknown) => {
      dispatch({ type: "request_resolving", requestId });
      try {
        await agentRuntime.resolveRequest(requestId, result);
        dispatch({ type: "request_resolved", requestId });
      } catch (error) {
        dispatch({
          type: "request_failed",
          requestId,
          error: errorMessage(error),
        });
      }
    },
    [],
  );

  const denyRequest = useCallback(async (requestId: RequestId) => {
    dispatch({ type: "request_resolving", requestId });
    try {
      await agentRuntime.rejectRequest(requestId, "Denied by the user.");
      dispatch({ type: "request_resolved", requestId });
    } catch (error) {
      dispatch({
        type: "request_failed",
        requestId,
        error: errorMessage(error),
      });
    }
  }, []);

  const executeDynamicTool = useCallback(
    async (request: DynamicToolRequest) => {
      dispatch({ type: "request_resolving", requestId: request.id });
      try {
        await executeUserApprovedDynamicTool(request);
        dispatch({ type: "request_resolved", requestId: request.id });
        return true;
      } catch (error) {
        dispatch({
          type: "request_failed",
          requestId: request.id,
          error: errorMessage(error),
        });
        return false;
      }
    },
    [],
  );

  const exportSessionArchive = useCallback(async () => {
    const threads = await Promise.all(
      stateRef.current.threads.map(async (thread) => {
        const response = await agentRuntime.request("thread/read", {
          threadId: thread.id,
          includeTurns: true,
        });
        return response.thread;
      }),
    );
    return createConversationArchive(
      "session",
      threads.map((thread) => ({
        id: thread.id,
        updatedAt: new Date(thread.updatedAt * 1_000).toISOString(),
        messages: runtimeMessagesFromThread(thread).map((message) => ({
          id: message.id,
          role: message.role,
          content: message.content,
          error: message.error,
          validation: message.validation,
        })),
      })),
      stateRef.current.activity.map((entry) => ({
        timestamp: entry.timestamp.toISOString(),
        kind: entry.kind,
        message: entry.message,
      })),
    );
  }, []);

  const exportCurrentThreadArchive = useCallback(() => {
    const thread = stateRef.current.activeThread;
    if (!thread) {
      return null;
    }
    return createConversationArchive("conversation", [
      {
        id: thread.id,
        updatedAt: new Date(thread.updatedAt * 1_000).toISOString(),
        messages: stateRef.current.messages.map((message) => ({
          id: message.id,
          role: message.role,
          content: message.content,
          error: message.error,
          validation: message.validation,
        })),
      },
    ]);
  }, []);

  const threadSummaries = useMemo(
    () => runtimeThreadSummaries(state.threads),
    [state.threads],
  );
  const resetSession = useCallback(
    () => dispatch({ type: "session_reset" }),
    [],
  );
  const clearActivity = useCallback(
    () => dispatch({ type: "activity_cleared" }),
    [],
  );

  return {
    ...state,
    threadSummaries,
    activeThreadId: state.activeThread?.id ?? "",
    startThread,
    resumeThread,
    forkThread,
    archiveThread,
    nameThread,
    sendPrompt,
    retryLastPrompt,
    interruptTurn,
    refreshThreads,
    resolveRequest,
    denyRequest,
    executeDynamicTool,
    exportSessionArchive,
    exportCurrentThreadArchive,
    clearActivity,
    resetSession,
  };
}
