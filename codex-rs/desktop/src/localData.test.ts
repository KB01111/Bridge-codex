import { beforeEach, describe, expect, it } from "vitest";

import {
  LEGACY_WORKSPACE_STORAGE_KEY,
  LOCAL_PREFERENCES_STORAGE_KEY,
  clearLegacyData,
  createLegacyDataArchive,
  detectLegacyData,
  loadLocalPreferences,
  saveLocalPreferences,
  type LocalPreferencesV2,
} from "./localData";

describe("local browser data", () => {
  beforeEach(() => window.localStorage.clear());

  it("persists only the versioned preference allowlist", () => {
    const unsafePreferences: LocalPreferencesV2 & {
      conversations: Array<{ content: string }>;
    } = {
      version: 2,
      theme: "dark",
      navigationCollapsed: true,
      conversations: [{ content: "secret prompt" }],
    };
    saveLocalPreferences(unsafePreferences);

    expect(
      JSON.parse(
        window.localStorage.getItem(LOCAL_PREFERENCES_STORAGE_KEY) ?? "{}",
      ),
    ).toEqual({ version: 2, theme: "dark", navigationCollapsed: true });
    expect(window.localStorage.getItem(LOCAL_PREFERENCES_STORAGE_KEY)).not.toContain(
      "secret prompt",
    );
  });

  it("defaults to the system theme without persisting sensitive state", () => {
    expect(loadLocalPreferences()).toEqual({
      version: 2,
      theme: "system",
      navigationCollapsed: false,
    });
  });

  it("summarizes, exports, and removes legacy conversations", () => {
    window.localStorage.setItem(
      LEGACY_WORKSPACE_STORAGE_KEY,
      JSON.stringify({
        conversations: [
          { id: "one", chat: [{ content: "private message" }] },
        ],
      }),
    );

    expect(detectLegacyData()).toMatchObject({
      conversationCount: 1,
      messageCount: 1,
    });
    expect(createLegacyDataArchive()).toContain("private message");
    clearLegacyData();
    expect(window.localStorage.getItem(LEGACY_WORKSPACE_STORAGE_KEY)).toBeNull();
  });
});
