import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useReducer } from "react";
import { describe, expect, it, vi } from "vitest";

import {
  createLocalDataState,
  localDataReducer,
} from "../localDataReducer";
import { LocalDataPanel } from "./LocalDataPanel";

function TestPanel({
  onArchiveCurrent = vi.fn(),
  onDeleteAll = vi.fn(async () => true),
}: {
  onArchiveCurrent?: () => void;
  onDeleteAll?: () => Promise<boolean>;
}) {
  const [state, dispatch] = useReducer(
    localDataReducer,
    createLocalDataState(null),
  );
  return (
    <LocalDataPanel
      state={state}
      dispatch={dispatch}
      conversationCount={2}
      currentMessageCount={3}
      busy={false}
      onExportSession={vi.fn()}
      onArchiveCurrent={onArchiveCurrent}
      onDeleteAll={onDeleteAll}
      onExportSupportBundle={vi.fn(async () => "C:\\support.zip")}
    />
  );
}

describe("LocalDataPanel", () => {
  it("archives the current conversation from the explicit action", async () => {
    const user = userEvent.setup();
    const onArchiveCurrent = vi.fn();
    render(<TestPanel onArchiveCurrent={onArchiveCurrent} />);

    await user.click(
      screen.getByRole("button", { name: "Archive current conversation" }),
    );
    expect(onArchiveCurrent).toHaveBeenCalledOnce();
  });

  it("requires the full confirmation phrase before deleting", async () => {
    const user = userEvent.setup();
    const onDeleteAll = vi.fn(async () => true);
    render(<TestPanel onDeleteAll={onDeleteAll} />);

    await user.click(
      screen.getByRole("button", { name: "Delete all local data" }),
    );
    const confirm = screen.getByRole("button", {
      name: "Delete all local data",
    });
    expect(confirm).toBeDisabled();
    await user.type(
      screen.getByLabelText(/type delete all to confirm/i),
      "DELETE ALL",
    );
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(onDeleteAll).toHaveBeenCalledOnce();
    expect(
      await screen.findByText("All Bridge-owned local data was deleted."),
    ).toBeVisible();
  });
});
