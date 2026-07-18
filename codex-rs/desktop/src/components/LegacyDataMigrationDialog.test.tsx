import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useReducer } from "react";
import { describe, expect, it, vi } from "vitest";

import {
  createLocalDataState,
  localDataReducer,
} from "../localDataReducer";
import { LegacyDataMigrationDialog } from "./LegacyDataMigrationDialog";

function TestDialog({ onClear }: { onClear: () => void }) {
  const [state, dispatch] = useReducer(
    localDataReducer,
    createLocalDataState({
      storageKeys: ["bridge-codex.workspace.v2"],
      byteLength: 1_024,
      conversationCount: 1,
      messageCount: 3,
    }),
  );
  return (
    <LegacyDataMigrationDialog
      state={state}
      onExport={() => dispatch({ type: "legacy_exported" })}
      onClear={onClear}
    />
  );
}

describe("LegacyDataMigrationDialog", () => {
  it("requires an archive download before removal", async () => {
    const user = userEvent.setup();
    const onClear = vi.fn();
    render(<TestDialog onClear={onClear} />);

    const remove = screen.getByRole("button", {
      name: "Remove saved browser copy",
    });
    expect(remove).toBeDisabled();
    await user.click(
      screen.getByRole("button", { name: "Download conversation archive" }),
    );
    expect(remove).toBeEnabled();
    await user.click(remove);
    expect(onClear).toHaveBeenCalledOnce();
  });
});
