import { afterEach, describe, expect, it, vi } from "vitest";

import type { ServerRequest } from "../../app-server-protocol/schema/typescript/ServerRequest";
import { agentRuntime } from "./agentRuntime";
import { backend } from "./backend";
import {
  executeUserApprovedDynamicTool,
  hydratePendingServerRequests,
} from "./useAgentRuntime";

type DynamicToolRequest = Extract<
  ServerRequest,
  { method: "item/tool/call" }
>;

function dynamicToolRequest(tool: string): DynamicToolRequest {
  return {
    id: 41,
    method: "item/tool/call",
    params: {
      threadId: "thread-1",
      turnId: "turn-1",
      callId: "call-1",
      namespace: "bridge",
      tool,
      arguments: {},
    },
  };
}

describe("executeUserApprovedDynamicTool", () => {
  afterEach(() => vi.restoreAllMocks());

  it("grants per-thread browser consent before executing an approved browser tool", async () => {
    const callOrder: string[] = [];
    vi.spyOn(backend, "grantAgentBrowserConsent").mockImplementation(
      async () => {
        callOrder.push("grant");
      },
    );
    vi.spyOn(agentRuntime, "executeDynamicTool").mockImplementation(
      async () => {
        callOrder.push("execute");
        return { success: true, contentItems: [] };
      },
    );
    vi.spyOn(agentRuntime, "resolveRequest").mockImplementation(async () => {
      callOrder.push("resolve");
    });

    await executeUserApprovedDynamicTool(dynamicToolRequest("browser_navigate"));

    expect(callOrder).toEqual(["grant", "execute", "resolve"]);
    expect(backend.grantAgentBrowserConsent).toHaveBeenCalledWith("thread-1");
  });

  it("does not grant browser consent for an approved non-browser tool", async () => {
    const grantConsent = vi
      .spyOn(backend, "grantAgentBrowserConsent")
      .mockResolvedValue();
    vi.spyOn(agentRuntime, "executeDynamicTool").mockResolvedValue({
      success: true,
      contentItems: [],
    });
    vi.spyOn(agentRuntime, "resolveRequest").mockResolvedValue();

    await executeUserApprovedDynamicTool(dynamicToolRequest("memory_search"));

    expect(grantConsent).not.toHaveBeenCalled();
  });
});

describe("hydratePendingServerRequests", () => {
  afterEach(() => vi.restoreAllMocks());

  it("replays retained native requests through the live request receiver", async () => {
    const request: ServerRequest = {
      id: 83,
      method: "item/fileChange/requestApproval",
      params: {
        threadId: "thread-1",
        turnId: "turn-1",
        itemId: "item-1",
        startedAtMs: 1,
        reason: "Apply the pending diff",
        grantRoot: null,
      },
    };
    vi.spyOn(agentRuntime, "listPendingRequests").mockResolvedValue([
      { requestId: 83, payload: request },
    ]);
    const receive = vi.fn();

    await hydratePendingServerRequests(receive);

    expect(receive).toHaveBeenCalledWith(request);
  });
});
