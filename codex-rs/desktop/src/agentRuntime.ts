import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

import type { RequestId } from "../../app-server-protocol/schema/typescript/RequestId";
import type { ServerNotification } from "../../app-server-protocol/schema/typescript/ServerNotification";
import type { ServerRequest } from "../../app-server-protocol/schema/typescript/ServerRequest";
import type { DynamicToolCallParams } from "../../app-server-protocol/schema/typescript/v2/DynamicToolCallParams";
import type { DynamicToolCallResponse } from "../../app-server-protocol/schema/typescript/v2/DynamicToolCallResponse";
import type { ModelListParams } from "../../app-server-protocol/schema/typescript/v2/ModelListParams";
import type { ModelListResponse } from "../../app-server-protocol/schema/typescript/v2/ModelListResponse";
import type { ThreadArchiveParams } from "../../app-server-protocol/schema/typescript/v2/ThreadArchiveParams";
import type { ThreadArchiveResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadArchiveResponse";
import type { ThreadDeleteParams } from "../../app-server-protocol/schema/typescript/v2/ThreadDeleteParams";
import type { ThreadDeleteResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadDeleteResponse";
import type { ThreadForkParams } from "../../app-server-protocol/schema/typescript/v2/ThreadForkParams";
import type { ThreadForkResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadForkResponse";
import type { ThreadListParams } from "../../app-server-protocol/schema/typescript/v2/ThreadListParams";
import type { ThreadListResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadListResponse";
import type { ThreadReadParams } from "../../app-server-protocol/schema/typescript/v2/ThreadReadParams";
import type { ThreadReadResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadReadResponse";
import type { ThreadResumeParams } from "../../app-server-protocol/schema/typescript/v2/ThreadResumeParams";
import type { ThreadResumeResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadResumeResponse";
import type { ThreadSetNameParams } from "../../app-server-protocol/schema/typescript/v2/ThreadSetNameParams";
import type { ThreadSetNameResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadSetNameResponse";
import type { ThreadStartParams } from "../../app-server-protocol/schema/typescript/v2/ThreadStartParams";
import type { ThreadStartResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadStartResponse";
import type { ThreadUnarchiveParams } from "../../app-server-protocol/schema/typescript/v2/ThreadUnarchiveParams";
import type { ThreadUnarchiveResponse } from "../../app-server-protocol/schema/typescript/v2/ThreadUnarchiveResponse";
import type { TurnInterruptParams } from "../../app-server-protocol/schema/typescript/v2/TurnInterruptParams";
import type { TurnInterruptResponse } from "../../app-server-protocol/schema/typescript/v2/TurnInterruptResponse";
import type { TurnStartParams } from "../../app-server-protocol/schema/typescript/v2/TurnStartParams";
import type { TurnStartResponse } from "../../app-server-protocol/schema/typescript/v2/TurnStartResponse";
import type { TurnSteerParams } from "../../app-server-protocol/schema/typescript/v2/TurnSteerParams";
import type { TurnSteerResponse } from "../../app-server-protocol/schema/typescript/v2/TurnSteerResponse";
import { previewInvoke, previewListen, previewMode } from "./previewBridge";

function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return previewMode ? previewInvoke<T>(command, args) : tauriInvoke<T>(command, args);
}

function listen<T>(
  event: string,
  handler: Parameters<typeof tauriListen<T>>[1],
): Promise<UnlistenFn> {
  return previewMode ? previewListen() : tauriListen<T>(event, handler);
}

export type BridgeAgentRuntimeStatus = {
  running: boolean;
  authenticated: boolean;
  responsesApi: boolean;
  providerBaseUrl?: string | null;
  error?: string | null;
};

export type BridgeAgentEvent =
  | { kind: "serverNotification"; payload: ServerNotification }
  | { kind: "serverRequest"; payload: ServerRequest }
  | { kind: "lagged"; skipped: number }
  | { kind: "disconnected"; message: string };

export type BridgePendingServerRequest = {
  requestId: RequestId;
  payload: ServerRequest;
};

type AgentRequestMap = {
  "thread/list": [ThreadListParams, ThreadListResponse];
  "thread/read": [ThreadReadParams, ThreadReadResponse];
  "thread/start": [ThreadStartParams, ThreadStartResponse];
  "thread/resume": [ThreadResumeParams, ThreadResumeResponse];
  "thread/fork": [ThreadForkParams, ThreadForkResponse];
  "thread/archive": [ThreadArchiveParams, ThreadArchiveResponse];
  "thread/unarchive": [ThreadUnarchiveParams, ThreadUnarchiveResponse];
  "thread/delete": [ThreadDeleteParams, ThreadDeleteResponse];
  "thread/name/set": [ThreadSetNameParams, ThreadSetNameResponse];
  "turn/start": [TurnStartParams, TurnStartResponse];
  "turn/steer": [TurnSteerParams, TurnSteerResponse];
  "turn/interrupt": [TurnInterruptParams, TurnInterruptResponse];
  "model/list": [ModelListParams, ModelListResponse];
};

export type AgentMethod = keyof AgentRequestMap;
export type AgentParams<M extends AgentMethod> = AgentRequestMap[M][0];
export type AgentResponse<M extends AgentMethod> = AgentRequestMap[M][1];

export const agentRuntime = {
  status: () =>
    invoke<BridgeAgentRuntimeStatus>("get_agent_runtime_status"),
  ensureStarted: () =>
    invoke<BridgeAgentRuntimeStatus>("ensure_agent_runtime"),
  listPendingRequests: () =>
    invoke<BridgePendingServerRequest[]>("list_pending_agent_requests"),
  request: <M extends AgentMethod>(method: M, params: AgentParams<M>) =>
    invoke<AgentResponse<M>>("agent_request", {
      request: { method, params },
    }),
  resolveRequest: (requestId: RequestId, result: unknown) =>
    invoke<void>("resolve_agent_request", { requestId, result }),
  rejectRequest: (requestId: RequestId, message: string) =>
    invoke<void>("reject_agent_request", { requestId, message }),
  executeDynamicTool: (request: DynamicToolCallParams) =>
    invoke<DynamicToolCallResponse>("execute_agent_dynamic_tool", { request }),
  onEvent: (handler: (event: BridgeAgentEvent) => void): Promise<UnlistenFn> =>
    listen<BridgeAgentEvent>("agent-event", ({ payload }) => handler(payload)),
  onStatus: (
    handler: (status: BridgeAgentRuntimeStatus) => void,
  ): Promise<UnlistenFn> =>
    listen<BridgeAgentRuntimeStatus>("agent-status", ({ payload }) =>
      handler(payload),
    ),
};
