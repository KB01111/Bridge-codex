export const LOCAL_PREFERENCES_STORAGE_KEY = "bridge-codex.preferences.v2";
export const LEGACY_WORKSPACE_STORAGE_KEY = "bridge-codex.workspace.v2";
export const LEGACY_SESSION_STORAGE_KEY = "bridge-codex.session.v1";

const LEGACY_THEME_STORAGE_KEY = "bridge-codex-theme";
const PREVIOUS_PREFERENCES_STORAGE_KEY = "bridge-codex.preferences.v1";

export type ThemePreference = "light" | "dark" | "system";

export type LocalPreferencesV2 = {
  version: 2;
  theme: ThemePreference;
  navigationCollapsed: boolean;
};

export type LegacyDataSummary = {
  storageKeys: string[];
  byteLength: number;
  conversationCount: number;
  messageCount: number;
};

export type ArchiveMessage = {
  id: string;
  role: string;
  content: string;
  error?: string | null;
  validation?: unknown;
};

export type ArchiveConversation = {
  id: string;
  updatedAt: string;
  messages: ArchiveMessage[];
};

export type ArchiveTraceEntry = {
  timestamp: string;
  kind: string;
  message: string;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function loadLocalPreferences(): LocalPreferencesV2 {
  const fallback: LocalPreferencesV2 = {
    version: 2,
    theme: "system",
    navigationCollapsed: false,
  };
  try {
    const raw = window.localStorage.getItem(LOCAL_PREFERENCES_STORAGE_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (
        isRecord(parsed) &&
        parsed.version === 2 &&
        (parsed.theme === "light" ||
          parsed.theme === "dark" ||
          parsed.theme === "system") &&
        typeof parsed.navigationCollapsed === "boolean"
      ) {
        return {
          version: 2,
          theme: parsed.theme,
          navigationCollapsed: parsed.navigationCollapsed,
        };
      }
    }

    const previousRaw = window.localStorage.getItem(
      PREVIOUS_PREFERENCES_STORAGE_KEY,
    );
    if (previousRaw) {
      const previous: unknown = JSON.parse(previousRaw);
      if (
        isRecord(previous) &&
        previous.version === 1 &&
        (previous.theme === "light" || previous.theme === "dark") &&
        typeof previous.navigationCollapsed === "boolean"
      ) {
        return {
          version: 2,
          theme: previous.theme,
          navigationCollapsed: previous.navigationCollapsed,
        };
      }
    }

    const legacyTheme = window.localStorage.getItem(LEGACY_THEME_STORAGE_KEY);
    if (legacyTheme === "light" || legacyTheme === "dark") {
      return { ...fallback, theme: legacyTheme };
    }
  } catch {
    // Browser storage is optional; use accessible system defaults when blocked.
  }
  return fallback;
}

export function saveLocalPreferences(preferences: LocalPreferencesV2): void {
  try {
    window.localStorage.setItem(
      LOCAL_PREFERENCES_STORAGE_KEY,
      JSON.stringify({
        version: 2,
        theme: preferences.theme,
        navigationCollapsed: preferences.navigationCollapsed,
      } satisfies LocalPreferencesV2),
    );
    window.localStorage.removeItem(PREVIOUS_PREFERENCES_STORAGE_KEY);
    window.localStorage.removeItem(LEGACY_THEME_STORAGE_KEY);
  } catch {
    // The active preferences still work when browser storage is unavailable.
  }
}

function countMessages(value: unknown): number {
  if (!isRecord(value)) {
    return 0;
  }
  if (Array.isArray(value.chat)) {
    return value.chat.length;
  }
  return 0;
}

export function detectLegacyData(): LegacyDataSummary | null {
  try {
    const sources = [
      LEGACY_WORKSPACE_STORAGE_KEY,
      LEGACY_SESSION_STORAGE_KEY,
    ].flatMap((storageKey) => {
      const raw = window.localStorage.getItem(storageKey);
      return raw ? [{ storageKey, raw }] : [];
    });
    if (sources.length === 0) {
      return null;
    }

    let conversationCount = 0;
    let messageCount = 0;
    for (const source of sources) {
      try {
        const parsed: unknown = JSON.parse(source.raw);
        if (
          source.storageKey === LEGACY_WORKSPACE_STORAGE_KEY &&
          isRecord(parsed) &&
          Array.isArray(parsed.conversations)
        ) {
          conversationCount += parsed.conversations.length;
          messageCount += parsed.conversations.reduce(
            (total, conversation) => total + countMessages(conversation),
            0,
          );
        } else {
          conversationCount += 1;
          messageCount += countMessages(parsed);
        }
      } catch {
        // Invalid legacy data is still exportable verbatim before removal.
      }
    }

    return {
      storageKeys: sources.map(({ storageKey }) => storageKey),
      byteLength: sources.reduce(
        (total, { raw }) => total + new TextEncoder().encode(raw).byteLength,
        0,
      ),
      conversationCount,
      messageCount,
    };
  } catch {
    return null;
  }
}

export function createLegacyDataArchive(): string {
  const sources = [
    LEGACY_WORKSPACE_STORAGE_KEY,
    LEGACY_SESSION_STORAGE_KEY,
  ].flatMap((storageKey) => {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) {
      return [];
    }
    try {
      return [{ storageKey, encoding: "json", value: JSON.parse(raw) }];
    } catch {
      return [{ storageKey, encoding: "text", value: raw }];
    }
  });
  return JSON.stringify(
    {
      schema: "bridge-codex.legacy-browser-data",
      version: 1,
      exportedAt: new Date().toISOString(),
      sources,
    },
    null,
    2,
  );
}

export function clearLegacyData(): void {
  window.localStorage.removeItem(LEGACY_WORKSPACE_STORAGE_KEY);
  window.localStorage.removeItem(LEGACY_SESSION_STORAGE_KEY);
}

export function clearAllFrontendData(): void {
  clearLegacyData();
  window.localStorage.removeItem(LOCAL_PREFERENCES_STORAGE_KEY);
  window.localStorage.removeItem(PREVIOUS_PREFERENCES_STORAGE_KEY);
  window.localStorage.removeItem(LEGACY_THEME_STORAGE_KEY);
}

export function createConversationArchive(
  scope: "conversation" | "session",
  conversations: ArchiveConversation[],
  trace: ArchiveTraceEntry[] = [],
): string {
  return JSON.stringify(
    {
      schema: "bridge-codex.conversation-archive",
      version: 1,
      scope,
      exportedAt: new Date().toISOString(),
      conversations,
      trace,
    },
    null,
    2,
  );
}

export function downloadJsonArchive(contents: string, filename: string): void {
  const blob = new Blob([contents], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

export function archiveFilename(label: string): string {
  const timestamp = new Date().toISOString().replaceAll(":", "-");
  return `bridge-codex-${label}-${timestamp}.json`;
}
