import type { UnlistenFn } from "@tauri-apps/api/event";

import type { BrowserStatus } from "./types";

export const previewMode =
  (import.meta.env.DEV || import.meta.env.MODE === "preview") &&
  new URLSearchParams(window.location.search).get("preview") === "1";

let previewBrowserStatus: BrowserStatus = {
  running: false,
  url: null,
  viewportWidth: 1280,
  viewportHeight: 720,
  health: "stopped",
  error: null,
};

let previewA2aStatus = {
  running: false,
  enabled: false,
  address: "127.0.0.1:8120",
  tokenConfigured: false,
  error: null,
};

export async function previewInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  let result: unknown;
  switch (command) {
    case "get_proxy_status":
    case "configure_proxy":
      result = {
        running: true,
        error: null,
        baseUrl: "http://127.0.0.1:8317/",
        authenticated: true,
        responsesApi: true,
        compatibility: "conformant",
        probedModel: "gpt-5.2-codex",
        experimentalModelCount: 1,
      };
      break;
    case "fetch_active_models":
      result = [
        {
          id: "gpt-5.2-codex",
          object: "model",
          ownedBy: "openai",
          classification: "known",
        },
        {
          id: "local-experimental",
          object: "model",
          ownedBy: "local",
          classification: "experimental",
        },
      ];
      break;
    case "get_code_policy_status":
      result = {
        parser: "tree-sitter",
        languageAbiVersion: 15,
        supportedLanguages: ["Rust", "TypeScript", "Python"],
        compiledTarget: "x86_64-pc-windows-msvc",
        networkAccess: "denied",
        sandboxRoot: "workspace-write",
        responseContract: "preview warning",
      };
      break;
    case "browser_status":
      result = previewBrowserStatus;
      break;
    case "browser_start":
      previewBrowserStatus = {
        ...previewBrowserStatus,
        running: true,
        health: "running",
      };
      result = previewBrowserStatus;
      break;
    case "browser_stop":
      previewBrowserStatus = {
        ...previewBrowserStatus,
        running: false,
        health: "stopped",
      };
      result = previewBrowserStatus;
      break;
    case "browser_navigate":
      previewBrowserStatus = {
        ...previewBrowserStatus,
        running: true,
        health: "running",
        url: typeof args?.url === "string" ? args.url : previewBrowserStatus.url,
      };
      result = undefined;
      break;
    case "browser_click_selector":
    case "browser_click_at":
    case "browser_type":
    case "desktop_click":
    case "desktop_type":
    case "delete_all_local_data":
    case "grant_agent_browser_consent":
    case "revoke_agent_browser_consent":
      result = undefined;
      break;
    case "desktop_status":
    case "enable_desktop_work_mode":
    case "disable_desktop_work_mode":
      result = {
        available: true,
        enabled: command === "enable_desktop_work_mode",
        platform: "windows",
        displaySize: [1920, 1080],
        error: null,
      };
      break;
    case "get_a2a_status":
      result = previewA2aStatus;
      break;
    case "configure_a2a_server": {
      const settings = args?.settings as
        | { enabled?: boolean; port?: number }
        | undefined;
      const enabled = Boolean(settings?.enabled);
      const port = settings?.port ?? 8120;
      previewA2aStatus = {
        ...previewA2aStatus,
        enabled,
        running: enabled && previewA2aStatus.tokenConfigured,
        address: `127.0.0.1:${port}`,
      };
      result = previewA2aStatus;
      break;
    }
    case "generate_a2a_token":
    case "regenerate_a2a_token":
      previewA2aStatus = { ...previewA2aStatus, tokenConfigured: true };
      result = {
        token: "bridge-preview-token-shown-once",
        status: previewA2aStatus,
      };
      break;
    case "delete_a2a_token":
      previewA2aStatus = {
        ...previewA2aStatus,
        running: false,
        tokenConfigured: false,
      };
      result = previewA2aStatus;
      break;
    case "list_a2a_tasks":
      result = [];
      break;
    case "get_code_memory_status":
    case "clear_code_memory":
      result = {
        ready: false,
        indexing: false,
        root: null,
        indexedAt: null,
        schemaVersion: 1,
        parser: "tree-sitter",
        retrieval: "hybrid",
        storage: "local",
        networkAccess: "denied",
        statistics: {
          discoveredEntries: 0,
          discoveredFiles: 0,
          indexedFiles: 0,
          skippedFiles: 0,
          sourceBytes: 0,
          chunks: 0,
          syntaxIssues: 0,
          symbols: 0,
          graphEdges: 0,
        },
        error: null,
      };
      break;
    case "search_code_memory":
      result = [];
      break;
    case "export_support_bundle":
      result = "C:\\Users\\Bridge\\support\\bridge-support.zip";
      break;
    case "get_agent_runtime_status":
    case "ensure_agent_runtime":
      result = {
        running: true,
        authenticated: true,
        responsesApi: true,
        providerBaseUrl: "http://127.0.0.1:8317/",
        error: null,
      };
      break;
    case "list_pending_agent_requests":
      result = [];
      break;
    case "agent_request": {
      const request = args?.request as { method?: string } | undefined;
      if (request?.method === "thread/list") {
        result = { data: [], nextCursor: null };
        break;
      }
      throw new Error(`Preview mode does not implement ${request?.method ?? "this request"}.`);
    }
    default:
      throw new Error(`Preview mode does not implement ${command}.`);
  }
  return result as T;
}

export function previewListen(): Promise<UnlistenFn> {
  return Promise.resolve(() => undefined);
}
