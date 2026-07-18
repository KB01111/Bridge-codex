import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { createLocalDataState } from "../localDataReducer";
import { RoutingPanel, type RoutingPanelProps } from "./RoutingPanel";

function renderPanel(overrides: Partial<RoutingPanelProps> = {}) {
  const props: RoutingPanelProps = {
    proxyStatus: null,
    codePolicyStatus: null,
    models: [],
    selectedModel: "",
    runtimeStatus: null,
    busy: false,
    onSelectModel: vi.fn(),
    onRefreshModels: vi.fn(async () => {}),
    onConfigureProxy: vi.fn(async () => {}),
    localDataState: createLocalDataState(null),
    dispatchLocalData: vi.fn(),
    conversationCount: 0,
    currentMessageCount: 0,
    localDataBusy: false,
    onExportSession: vi.fn(),
    onArchiveCurrent: vi.fn(),
    onDeleteAllLocalData: vi.fn(async () => true),
    onExportSupportBundle: vi.fn(async () => "bundle.zip"),
    ...overrides,
  };
  render(<RoutingPanel {...props} />);
}

describe("RoutingPanel", () => {
  it.each([
    ["successful", vi.fn(async () => {})],
    [
      "failed",
      vi.fn(async () => {
        throw new Error("Capability probe failed.");
      }),
    ],
  ])("clears the API key after a %s connection attempt", async (_, connect) => {
    const user = userEvent.setup();
    renderPanel({ onConfigureProxy: connect });
    const apiKey = screen.getByLabelText("Proxy API key");

    await user.type(apiKey, "super-secret-key");
    await user.click(screen.getByRole("button", { name: "Connect proxy" }));

    await waitFor(() => expect(apiKey).toHaveValue(""));
    expect(connect).toHaveBeenCalledWith(
      "http://127.0.0.1:8317/",
      "super-secret-key",
    );
  });

  it("keeps reported but unprobed models visible and unavailable", async () => {
    const user = userEvent.setup();
    const onSelectModel = vi.fn();
    renderPanel({
      proxyStatus: {
        running: true,
        error: null,
        baseUrl: "http://127.0.0.1:8317/",
        authenticated: true,
        responsesApi: true,
        compatibility: "conformant",
        probedModel: "gpt-5.2-codex",
        experimentalModelCount: 1,
      },
      models: [
        { id: "gpt-5.2-codex", classification: "known" },
        { id: "local-experimental", classification: "experimental" },
      ],
      selectedModel: "gpt-5.2-codex",
      onSelectModel,
    });

    await user.click(screen.getByRole("button", { name: "Choose active model" }));

    expect(
      screen.getByRole("menuitemradio", { name: /local-experimental/i }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.getByText("Experimental · unavailable · not conformance tested"),
    ).toBeInTheDocument();
    expect(onSelectModel).not.toHaveBeenCalled();
  });
});
