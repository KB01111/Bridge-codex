import type { RequestId } from "../../app-server-protocol/schema/typescript/RequestId";
import type { ServerNotification } from "../../app-server-protocol/schema/typescript/ServerNotification";
import type { ServerRequest } from "../../app-server-protocol/schema/typescript/ServerRequest";
import type { Thread } from "../../app-server-protocol/schema/typescript/v2/Thread";
import type { ThreadItem } from "../../app-server-protocol/schema/typescript/v2/ThreadItem";

import type { BridgeAgentRuntimeStatus } from "./agentRuntime";
import type { TraceEntry, UiChatMessage } from "./workbenchTypes";

const MAX_RUNTIME_MESSAGES = 240;
const MAX_RUNTIME_MESSAGE_LENGTH = 64 * 1024;
const MAX_RUNTIME_ACTIVITY = 240;

export type RuntimeThreadSummary = {
  id: string;
  title: string;
  updatedAt: string;
  messageCount: number;
};

export type PendingServerRequest = {
  request: ServerRequest;
  error: string | null;
  resolving: boolean;
};

export type AgentRuntimeState = {
  status: BridgeAgentRuntimeStatus | null;
  threads: Thread[];
  activeThread: Thread | null;
  activeTurnId: string | null;
  messages: UiChatMessage[];
  activity: TraceEntry[];
  pendingRequests: PendingServerRequest[];
  loading: boolean;
  sending: boolean;
  error: string | null;
  lagged: number;
};

export type AgentRuntimeAction =
  | { type: "loading"; loading: boolean }
  | { type: "status"; status: BridgeAgentRuntimeStatus }
  | { type: "failed"; error: string }
  | { type: "threads_loaded"; threads: Thread[] }
  | { type: "thread_loaded"; thread: Thread }
  | { type: "notification"; notification: ServerNotification }
  | { type: "request_received"; request: ServerRequest }
  | { type: "request_resolving"; requestId: RequestId }
  | { type: "request_failed"; requestId: RequestId; error: string }
  | { type: "request_resolved"; requestId: RequestId }
  | { type: "lagged"; skipped: number }
  | { type: "disconnected"; message: string }
  | { type: "local_user_message"; id: string; content: string }
  | { type: "activity_cleared" }
  | { type: "session_reset" };

export const initialAgentRuntimeState: AgentRuntimeState = {
  status: null,
  threads: [],
  activeThread: null,
  activeTurnId: null,
  messages: [],
  activity: [],
  pendingRequests: [],
  loading: true,
  sending: false,
  error: null,
  lagged: 0,
};

function requestKey(requestId: RequestId): string {
  return `${typeof requestId}:${String(requestId)}`;
}

function boundedText(value: string): string {
  if (value.length <= MAX_RUNTIME_MESSAGE_LENGTH) {
    return value;
  }
  return `${value.slice(0, MAX_RUNTIME_MESSAGE_LENGTH - 1)}…`;
}

function userInputText(item: Extract<ThreadItem, { type: "userMessage" }>): string {
  return item.content
    .map((input) => {
      switch (input.type) {
        case "text":
          return input.text;
        case "image":
          return `[Image: ${input.url}]`;
        case "localImage":
          return `[Local image: ${input.path}]`;
        case "skill":
          return `[Skill: ${input.name}]`;
        case "mention":
          return `@${input.name}`;
      }
    })
    .join("\n");
}

function json(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export function runtimeMessageFromItem(
  item: ThreadItem,
  streaming = false,
): UiChatMessage | null {
  switch (item.type) {
    case "userMessage":
      return {
        id: item.id,
        role: "user",
        content: boundedText(userInputText(item)),
        label: "You",
      };
    case "agentMessage":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText(item.text),
        streaming,
        label: "Bridge",
      };
    case "plan":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText(item.text),
        streaming,
        label: "Plan",
      };
    case "reasoning":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText([...item.summary, ...item.content].join("\n\n")),
        streaming,
        label: "Reasoning",
      };
    case "commandExecution":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText(
          `\`\`\`shell\n${item.command}\n\`\`\`\n\n${item.aggregatedOutput ?? ""}`,
        ),
        streaming,
        label: `Command · ${item.status}`,
      };
    case "fileChange":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText(
          item.changes
            .map(
              (change) =>
                `**${change.kind}: ${change.path}**\n\n\`\`\`diff\n${change.diff}\n\`\`\``,
            )
            .join("\n\n"),
        ),
        streaming,
        label: `File changes · ${item.status}`,
      };
    case "mcpToolCall":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText(
          `**${item.server} / ${item.tool}**\n\nArguments:\n\`\`\`json\n${json(item.arguments)}\n\`\`\`${item.result ? `\n\nResult:\n\`\`\`json\n${json(item.result)}\n\`\`\`` : ""}${item.error ? `\n\n${item.error.message}` : ""}`,
        ),
        streaming,
        label: `MCP tool · ${item.status}`,
      };
    case "dynamicToolCall":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText(
          `**${item.namespace ? `${item.namespace} / ` : ""}${item.tool}**\n\n\`\`\`json\n${json(item.arguments)}\n\`\`\`${item.contentItems ? `\n\n${json(item.contentItems)}` : ""}`,
        ),
        streaming,
        label: `Dynamic tool · ${item.status}`,
      };
    case "collabAgentToolCall":
      return {
        id: item.id,
        role: "assistant",
        content: boundedText(
          `${item.tool}: ${item.prompt ?? "No prompt"}\n\nTargets: ${item.receiverThreadIds.join(", ") || "pending"}`,
        ),
        streaming,
        label: `Subagent · ${item.status}`,
      };
    case "subAgentActivity":
      return {
        id: item.id,
        role: "assistant",
        content: `${item.kind} · ${item.agentPath}`,
        streaming,
        label: "Subagent activity",
      };
    case "hookPrompt":
    case "webSearch":
    case "imageView":
    case "sleep":
    case "imageGeneration":
    case "enteredReviewMode":
    case "exitedReviewMode":
    case "contextCompaction":
      return null;
  }
}

export function runtimeMessagesFromThread(thread: Thread): UiChatMessage[] {
  return thread.turns
    .flatMap((turn) =>
      turn.items.flatMap((item) => {
        const message = runtimeMessageFromItem(
          item,
          turn.status === "inProgress",
        );
        return message ? [message] : [];
      }),
    )
    .slice(-MAX_RUNTIME_MESSAGES);
}

function upsertMessage(
  messages: UiChatMessage[],
  message: UiChatMessage,
): UiChatMessage[] {
  const index = messages.findIndex((candidate) => candidate.id === message.id);
  if (index < 0) {
    return [...messages, message].slice(-MAX_RUNTIME_MESSAGES);
  }
  return messages.map((candidate) =>
    candidate.id === message.id ? message : candidate,
  );
}

function appendDelta(
  messages: UiChatMessage[],
  id: string,
  delta: string,
  label: string,
): UiChatMessage[] {
  const existing = messages.find((message) => message.id === id);
  return upsertMessage(messages, {
    id,
    role: "assistant",
    content: boundedText(`${existing?.content ?? ""}${delta}`),
    streaming: true,
    label: existing?.label ?? label,
  });
}

function addActivity(
  state: AgentRuntimeState,
  message: string,
  kind: TraceEntry["kind"] = "info",
): AgentRuntimeState {
  return {
    ...state,
    activity: [
      ...state.activity,
      {
        id: `runtime-${Date.now()}-${state.activity.length}`,
        timestamp: new Date(),
        kind,
        message: boundedText(message),
      },
    ].slice(-MAX_RUNTIME_ACTIVITY),
  };
}

function replaceThread(threads: Thread[], thread: Thread): Thread[] {
  return [thread, ...threads.filter((candidate) => candidate.id !== thread.id)];
}

function applyNotification(
  state: AgentRuntimeState,
  notification: ServerNotification,
): AgentRuntimeState {
  const { method, params } = notification;
  switch (method) {
    case "thread/started":
      return addActivity(
        { ...state, threads: replaceThread(state.threads, params.thread) },
        "Thread started",
        "success",
      );
    case "thread/name/updated":
      return {
        ...state,
        threads: state.threads.map((thread) =>
          thread.id === params.threadId
            ? { ...thread, name: params.threadName ?? null }
            : thread,
        ),
        activeThread:
          state.activeThread?.id === params.threadId
            ? { ...state.activeThread, name: params.threadName ?? null }
            : state.activeThread,
      };
    case "thread/archived":
    case "thread/deleted":
      return {
        ...state,
        threads: state.threads.filter(
          (thread) => thread.id !== params.threadId,
        ),
        activeThread:
          state.activeThread?.id === params.threadId
            ? null
            : state.activeThread,
      };
    case "turn/started":
      return addActivity(
        {
          ...state,
          activeTurnId: params.turn.id,
          sending: true,
          messages: params.turn.items.reduce((messages, item) => {
            const message = runtimeMessageFromItem(item, true);
            return message ? upsertMessage(messages, message) : messages;
          }, state.messages),
        },
        "Turn started",
      );
    case "turn/completed": {
      const completed = params.turn.items.reduce((messages, item) => {
        const message = runtimeMessageFromItem(item, false);
        return message ? upsertMessage(messages, message) : messages;
      }, state.messages);
      const failed = params.turn.status === "failed";
      return addActivity(
        {
          ...state,
          activeTurnId: null,
          sending: false,
          messages: completed,
          error: failed
            ? (params.turn.error?.message ?? "The turn failed.")
            : null,
        },
        failed ? "Turn failed" : `Turn ${params.turn.status}`,
        failed ? "error" : "success",
      );
    }
    case "item/started": {
      const message = runtimeMessageFromItem(params.item, true);
      return message
        ? { ...state, messages: upsertMessage(state.messages, message) }
        : state;
    }
    case "item/completed": {
      const message = runtimeMessageFromItem(params.item, false);
      return message
        ? { ...state, messages: upsertMessage(state.messages, message) }
        : state;
    }
    case "item/agentMessage/delta":
      return {
        ...state,
        messages: appendDelta(
          state.messages,
          params.itemId,
          params.delta,
          "Bridge",
        ),
      };
    case "item/reasoning/summaryTextDelta":
    case "item/reasoning/textDelta":
      return {
        ...state,
        messages: appendDelta(
          state.messages,
          params.itemId,
          params.delta,
          "Reasoning",
        ),
      };
    case "item/commandExecution/outputDelta":
      return {
        ...state,
        messages: appendDelta(
          state.messages,
          params.itemId,
          params.delta,
          "Command output",
        ),
      };
    case "item/fileChange/outputDelta":
      return {
        ...state,
        messages: appendDelta(
          state.messages,
          params.itemId,
          params.delta,
          "File changes",
        ),
      };
    case "item/fileChange/patchUpdated":
      return {
        ...state,
        messages: upsertMessage(state.messages, {
          id: params.itemId,
          role: "assistant",
          label: "File changes",
          streaming: true,
          content: boundedText(
            params.changes
              .map(
                (change) =>
                  `**${change.kind}: ${change.path}**\n\n\`\`\`diff\n${change.diff}\n\`\`\``,
              )
              .join("\n\n"),
          ),
        }),
      };
    case "item/mcpToolCall/progress":
      return {
        ...state,
        messages: appendDelta(
          state.messages,
          params.itemId,
          `${params.message}\n`,
          "MCP tool",
        ),
      };
    case "turn/diff/updated":
      return {
        ...state,
        messages: upsertMessage(state.messages, {
          id: `turn-diff-${params.turnId}`,
          role: "assistant",
          label: "Turn diff",
          content: boundedText(`\`\`\`diff\n${params.diff}\n\`\`\``),
        }),
      };
    case "thread/tokenUsage/updated":
      return {
        ...state,
        messages: upsertMessage(state.messages, {
          id: `usage-${params.turnId}`,
          role: "assistant",
          label: "Token usage",
          content: `\`\`\`json\n${json(params.tokenUsage)}\n\`\`\``,
        }),
      };
    case "error":
      return addActivity(
        {
          ...state,
          error: params.error.message,
          messages: upsertMessage(state.messages, {
            id: `error-${params.turnId}`,
            role: "assistant",
            label: params.willRetry ? "Retrying" : "Runtime error",
            content: "",
            error: params.error.message,
          }),
        },
        params.error.message,
        "error",
      );
    case "serverRequest/resolved":
      return {
        ...state,
        pendingRequests: state.pendingRequests.filter(
          ({ request }) => requestKey(request.id) !== requestKey(params.requestId),
        ),
      };
    case "thread/status/changed":
      return {
        ...state,
        threads: state.threads.map((thread) =>
          thread.id === params.threadId
            ? { ...thread, status: params.status }
            : thread,
        ),
      };
    case "thread/unarchived":
    case "thread/closed":
    case "skills/changed":
    case "thread/goal/updated":
    case "thread/goal/cleared":
    case "thread/environment/connected":
    case "thread/environment/disconnected":
    case "thread/settings/updated":
    case "hook/started":
    case "hook/completed":
    case "turn/plan/updated":
    case "item/plan/delta":
    case "item/autoApprovalReview/started":
    case "item/autoApprovalReview/completed":
    case "item/reasoning/summaryPartAdded":
    case "item/commandExecution/terminalInteraction":
    case "rawResponseItem/completed":
    case "rawResponse/completed":
    case "command/exec/outputDelta":
    case "process/outputDelta":
    case "process/exited":
    case "mcpServer/oauthLogin/completed":
    case "mcpServer/startupStatus/updated":
    case "account/updated":
    case "account/rateLimits/updated":
    case "app/list/updated":
    case "remoteControl/status/changed":
    case "externalAgentConfig/import/progress":
    case "externalAgentConfig/import/completed":
    case "fs/changed":
    case "thread/compacted":
    case "model/rerouted":
    case "model/verification":
    case "turn/moderationMetadata":
    case "model/safetyBuffering/updated":
    case "warning":
    case "guardianWarning":
    case "deprecationNotice":
    case "configWarning":
    case "fuzzyFileSearch/sessionUpdated":
    case "fuzzyFileSearch/sessionCompleted":
    case "thread/realtime/started":
    case "thread/realtime/itemAdded":
    case "thread/realtime/transcript/delta":
    case "thread/realtime/transcript/done":
    case "thread/realtime/outputAudio/delta":
    case "thread/realtime/sdp":
    case "thread/realtime/error":
    case "thread/realtime/closed":
    case "windows/worldWritableWarning":
    case "windowsSandbox/setupCompleted":
    case "account/login/completed":
      return state;
  }
}

export function agentRuntimeReducer(
  state: AgentRuntimeState,
  action: AgentRuntimeAction,
): AgentRuntimeState {
  switch (action.type) {
    case "loading":
      return { ...state, loading: action.loading };
    case "status":
      return { ...state, status: action.status, error: action.status.error ?? null };
    case "failed":
      return { ...state, loading: false, error: action.error };
    case "threads_loaded":
      return { ...state, loading: false, threads: action.threads, lagged: 0 };
    case "thread_loaded":
      return {
        ...state,
        activeThread: action.thread,
        activeTurnId:
          action.thread.turns.find((turn) => turn.status === "inProgress")?.id ??
          null,
        sending: action.thread.turns.some(
          (turn) => turn.status === "inProgress",
        ),
        messages: runtimeMessagesFromThread(action.thread),
        threads: replaceThread(state.threads, action.thread),
        error: null,
      };
    case "notification":
      return applyNotification(state, action.notification);
    case "request_received":
      return state.pendingRequests.some(
        ({ request }) => requestKey(request.id) === requestKey(action.request.id),
      )
        ? state
        : {
            ...state,
            pendingRequests: [
              ...state.pendingRequests,
              { request: action.request, error: null, resolving: false },
            ],
          };
    case "request_resolving":
      return {
        ...state,
        pendingRequests: state.pendingRequests.map((pending) =>
          requestKey(pending.request.id) === requestKey(action.requestId)
            ? { ...pending, resolving: true, error: null }
            : pending,
        ),
      };
    case "request_failed":
      return {
        ...state,
        pendingRequests: state.pendingRequests.map((pending) =>
          requestKey(pending.request.id) === requestKey(action.requestId)
            ? { ...pending, resolving: false, error: action.error }
            : pending,
        ),
      };
    case "request_resolved":
      return {
        ...state,
        pendingRequests: state.pendingRequests.filter(
          ({ request }) => requestKey(request.id) !== requestKey(action.requestId),
        ),
      };
    case "lagged":
      return addActivity(
        { ...state, lagged: state.lagged + action.skipped },
        `Runtime event stream lagged by ${action.skipped} events; refreshing thread state.`,
        "error",
      );
    case "disconnected":
      return addActivity(
        {
          ...state,
          pendingRequests: [],
          sending: false,
          error: action.message,
        },
        action.message,
        "error",
      );
    case "local_user_message":
      return {
        ...state,
        messages: upsertMessage(state.messages, {
          id: action.id,
          role: "user",
          content: action.content,
          label: "You",
        }),
      };
    case "activity_cleared":
      return { ...state, activity: [] };
    case "session_reset":
      return {
        ...initialAgentRuntimeState,
        status: state.status,
        loading: false,
      };
  }
}

export function runtimeThreadSummaries(
  threads: Thread[],
): RuntimeThreadSummary[] {
  return threads.map((thread) => ({
    id: thread.id,
    title: thread.name || thread.preview || "New task",
    updatedAt: new Date(thread.updatedAt * 1_000).toISOString(),
    messageCount: thread.turns.reduce(
      (total, turn) => total + turn.items.length,
      0,
    ),
  }));
}
