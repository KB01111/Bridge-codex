import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";

import type { StatusSidebarProps } from "./StatusSidebar";
import { StatusSidebar } from "./StatusSidebar";

function props(overrides: Partial<StatusSidebarProps> = {}): StatusSidebarProps {
  return {
    proxyStatus: null,
    browserStatus: null,
    a2aStatus: null,
    trace: [],
    chat: [{ id: "user-1", role: "user", content: "Ship this safely" }],
    activeThreadName: "",
    hasActiveThread: true,
    pendingRequests: [],
    sending: false,
    selectedModel: "local/model",
    onSend: vi.fn(async () => {}),
    onRetry: vi.fn(async () => {}),
    onInterrupt: vi.fn(async () => {}),
    onForkThread: vi.fn(async () => {}),
    onArchiveThread: vi.fn(async () => {}),
    onNameThread: vi.fn(async () => {}),
    onResolveRequest: vi.fn(async () => {}),
    onDenyRequest: vi.fn(async () => {}),
    onExecuteDynamicTool: vi.fn(async () => true),
    onClearTrace: vi.fn(),
    onOpenRouting: vi.fn(),
    ...overrides,
  };
}

describe("StatusSidebar", () => {
  it("requires confirmation before archiving a thread", async () => {
    const user = userEvent.setup();
    const onArchiveThread = vi.fn(async () => {});
    render(<StatusSidebar {...props({ onArchiveThread })} />);

    await user.click(
      screen.getByRole("button", { name: "Archive this thread" }),
    );
    expect(onArchiveThread).not.toHaveBeenCalled();
    expect(
      screen.getByRole("dialog", { name: "Archive this thread?" }),
    ).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "Archive thread" }),
    );
    expect(onArchiveThread).toHaveBeenCalledOnce();
  });

  it("announces when a streamed response completes", async () => {
    const view = render(<StatusSidebar {...props()} />);
    view.rerender(<StatusSidebar {...props({ sending: true })} />);
    view.rerender(<StatusSidebar {...props({ sending: false })} />);

    await waitFor(() => {
      expect(screen.getByText("Bridge response complete.")).toBeInTheDocument();
    });
    expect(screen.getByRole("log", { name: "Conversation messages" })).toBeVisible();
  });

  it("distinguishes disabled task delegation from an offline service", () => {
    render(
      <StatusSidebar
        {...props({
          a2aStatus: {
            enabled: false,
            running: false,
            address: "127.0.0.1:8120",
          },
        })}
      />,
    );

    expect(screen.getByRole("button", { name: /A2A: disabled/i })).toBeVisible();
  });

  it("has no detectable automated accessibility violations", async () => {
    const { container } = render(<StatusSidebar {...props()} />);
    const result = await axe.run(container, {
      rules: { "color-contrast": { enabled: false } },
    });
    expect(result.violations).toEqual([]);
  });
});
