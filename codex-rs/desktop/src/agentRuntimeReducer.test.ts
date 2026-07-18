import { describe, expect, it } from "vitest";

import {
  agentRuntimeReducer,
  initialAgentRuntimeState,
} from "./agentRuntimeReducer";

describe("agentRuntimeReducer", () => {
  it("drops approvals retained by a disconnected app-server instance", () => {
    const withPendingRequest = agentRuntimeReducer(initialAgentRuntimeState, {
      type: "request_received",
      request: {
        id: 1,
        method: "item/fileChange/requestApproval",
        params: {
          threadId: "thread-1",
          turnId: "turn-1",
          itemId: "item-1",
          startedAtMs: 1,
          reason: "Apply a diff",
          grantRoot: null,
        },
      },
    });

    const disconnected = agentRuntimeReducer(withPendingRequest, {
      type: "disconnected",
      message: "The embedded app-server stopped.",
    });

    expect(disconnected.pendingRequests).toEqual([]);
    expect(disconnected.error).toBe("The embedded app-server stopped.");
  });
});
