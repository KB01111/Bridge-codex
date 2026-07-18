import { describe, expect, it } from "vitest";

import {
  createLocalDataState,
  localDataReducer,
} from "./localDataReducer";

const legacyData = {
  storageKeys: ["bridge-codex.workspace.v2"],
  byteLength: 512,
  conversationCount: 2,
  messageCount: 7,
};

describe("localDataReducer", () => {
  it("requires a legacy export before the migration can be cleared", () => {
    const initial = createLocalDataState(legacyData);
    const exported = localDataReducer(initial, { type: "legacy_exported" });
    const cleared = localDataReducer(exported, { type: "legacy_cleared" });

    expect(exported.legacyExported).toBe(true);
    expect(cleared).toMatchObject({
      legacyData: null,
      legacyExported: false,
      notice: "Saved legacy conversations were exported and removed.",
    });
  });

  it("preserves an open confirmation when deletion fails", () => {
    const opened = localDataReducer(createLocalDataState(null), {
      type: "delete_opened",
    });
    const started = localDataReducer(opened, { type: "delete_started" });
    const failed = localDataReducer(started, {
      type: "delete_failed",
      error: "Native cleanup failed",
    });

    expect(failed).toMatchObject({
      deleteOpen: true,
      deleting: false,
      error: "Native cleanup failed",
    });
  });
});
