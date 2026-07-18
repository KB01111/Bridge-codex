import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PendingServerRequest } from "../agentRuntimeReducer";
import { ServerRequestQueue } from "./ServerRequestQueue";

function commandApproval(
  params: Record<string, unknown> = {},
): PendingServerRequest {
  return {
    resolving: false,
    error: null,
    request: {
      id: 7,
      method: "item/commandExecution/requestApproval",
      params: {
        threadId: "thread-1",
        turnId: "turn-1",
        itemId: "item-1",
        startedAtMs: 1,
        environmentId: null,
        reason: "Run the scoped command",
        command: "pnpm test",
        availableDecisions: ["accept", "cancel"],
        ...params,
      },
    } as PendingServerRequest["request"],
  };
}

function renderQueue(
  pending: PendingServerRequest,
  onResolve = vi.fn(async () => {}),
  onDeny = vi.fn(async () => {}),
) {
  render(
    <ServerRequestQueue
      requests={[pending]}
      onResolve={onResolve}
      onDeny={onDeny}
      onExecuteDynamicTool={vi.fn(async () => true)}
    />,
  );
  return { onResolve, onDeny };
}

describe("ServerRequestQueue", () => {
  it("offers only a one-time command approval", async () => {
    const user = userEvent.setup();
    const onResolve = vi.fn(async () => {});
    renderQueue(commandApproval(), onResolve);

    expect(screen.queryByText(/session/i)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Allow once" }));
    expect(onResolve).toHaveBeenCalledWith(7, { decision: "accept" });
  });

  it("shows the working directory, parsed actions, and additional permissions", () => {
    renderQueue(
      commandApproval({
        cwd: "C:\\work\\bridge",
        commandActions: [
          {
            type: "read",
            command: "Get-Content secrets.txt",
            name: "secrets.txt",
            path: "C:\\work\\bridge\\secrets.txt",
          },
        ],
        additionalPermissions: {
          network: { enabled: true },
          fileSystem: {
            read: ["C:\\sensitive"],
            write: ["C:\\exports"],
          },
        },
      }),
    );

    expect(screen.getByText("Working directory")).toBeInTheDocument();
    expect(screen.getByText("C:\\work\\bridge")).toBeInTheDocument();
    expect(screen.getByText("Parsed command actions")).toBeInTheDocument();
    expect(
      screen.getByText("Additional filesystem or network permissions"),
    ).toBeInTheDocument();
    expect(screen.getByText(/C:\\\\sensitive/)).toBeInTheDocument();
    expect(screen.getByText(/"enabled": true/)).toBeInTheDocument();
  });

  it("shows network scope and omits approval decisions Bridge does not support", async () => {
    const user = userEvent.setup();
    const onResolve = vi.fn(async () => {});
    renderQueue(
      commandApproval({
        command: null,
        networkApprovalContext: {
          host: "packages.example.test:443",
          protocol: "https",
        },
        availableDecisions: ["acceptForSession", "cancel"],
      }),
      onResolve,
    );

    expect(screen.getByText("Network destination")).toBeInTheDocument();
    expect(
      screen.getByText("https://packages.example.test:443"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Allow once" }),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Deny and stop turn" }),
    );
    expect(onResolve).toHaveBeenCalledWith(7, { decision: "cancel" });
  });

  it("defaults an unknown decision shape to rejection", async () => {
    const user = userEvent.setup();
    const onDeny = vi.fn(async () => {});
    renderQueue(
      commandApproval({ availableDecisions: [{ unexpected: true }] }),
      vi.fn(async () => {}),
      onDeny,
    );

    expect(
      screen.queryByRole("button", { name: "Allow once" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Deny" }));
    expect(onDeny).toHaveBeenCalledWith(7);
  });

  it("does not offer a session-scoped file-write grant", () => {
    renderQueue({
      resolving: false,
      error: null,
      request: {
        id: 12,
        method: "item/fileChange/requestApproval",
        params: {
          threadId: "thread-1",
          turnId: "turn-1",
          itemId: "item-1",
          startedAtMs: 1,
          reason: "Write generated files",
          grantRoot: "C:\\work\\bridge",
        },
      },
    });

    expect(screen.getByText(/session-scoped write grant/i)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Allow once" }),
    ).not.toBeInTheDocument();
  });

  it("submits free-form text when an option question allows Other", async () => {
    const user = userEvent.setup();
    const onResolve = vi.fn(async () => {});
    renderQueue(
      {
        resolving: false,
        error: null,
        request: {
          id: 19,
          method: "item/tool/requestUserInput",
          params: {
            threadId: "thread-1",
            turnId: "turn-1",
            itemId: "item-1",
            autoResolutionMs: null,
            questions: [
              {
                id: "scope",
                header: "Scope",
                question: "Which workspace should be used?",
                isOther: true,
                isSecret: false,
                options: [
                  {
                    label: "Current workspace",
                    description: "Use the repository already open.",
                  },
                ],
              },
            ],
          },
        },
      },
      onResolve,
    );

    await user.click(screen.getByRole("radio", { name: /Other/ }));
    await user.type(
      screen.getByLabelText("Other answer: Which workspace should be used?"),
      "C:\\work\\another",
    );
    await user.click(screen.getByRole("button", { name: "Send answer" }));

    expect(onResolve).toHaveBeenCalledWith(19, {
      answers: { scope: { answers: ["C:\\work\\another"] } },
    });
  });
});
