import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { clearBrowserFrame, publishBrowserFrame } from "../browserFrameStore";
import { BrowserWorkspace } from "./BrowserWorkspace";

describe("BrowserWorkspace", () => {
  it("offers keyboard coordinate interaction for the live preview", async () => {
    const onClickAt = vi.fn(async () => {});
    await act(async () => {
      publishBrowserFrame({
        jpegBase64: "",
        url: "https://example.com",
        width: 1280,
        height: 720,
        sequence: 1,
        capturedAt: "2026-07-16T12:00:00.000Z",
      });
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });
    render(
      <BrowserWorkspace
        status={{
          running: true,
          url: "https://example.com",
          viewportWidth: 1280,
          viewportHeight: 720,
          health: "running",
        }}
        busy={false}
        onStart={vi.fn(async () => {})}
        onStop={vi.fn(async () => {})}
        onNavigate={vi.fn(async () => {})}
        onClickSelector={vi.fn(async () => {})}
        onClickAt={onClickAt}
        onType={vi.fn(async () => {})}
      />,
    );

    const preview = await waitFor(() =>
      screen.getByRole("button", {
        name: /live agent browser preview/i,
      }),
    );
    expect(preview).toHaveAttribute("tabindex", "0");
    fireEvent.keyDown(preview, { key: "ArrowRight" });
    fireEvent.keyDown(preview, { key: "Enter" });
    expect(onClickAt).toHaveBeenCalledWith(650, 360);
    act(clearBrowserFrame);
  });
});
