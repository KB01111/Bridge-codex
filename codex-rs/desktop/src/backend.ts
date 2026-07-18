import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  A2aStatus,
  A2aServerSettings,
  A2aTask,
  A2aTokenProvisioning,
  BrowserFrame,
  BrowserStatus,
  CodePolicyStatus,
  CodeMemoryIndexResult,
  CodeMemorySearchRequest,
  CodeMemorySearchResult,
  CodeMemoryStatus,
  DelegateA2aTaskRequest,
  DesktopStatus,
  ProxyModel,
  ProxyStatus,
} from "./types";
import { previewInvoke, previewListen, previewMode } from "./previewBridge";

function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return previewMode ? previewInvoke<T>(command, args) : tauriInvoke<T>(command, args);
}

function listen<T>(
  event: string,
  handler: Parameters<typeof tauriListen<T>>[1],
): Promise<UnlistenFn> {
  return previewMode ? previewListen() : tauriListen<T>(event, handler);
}

type EventHandler<T> = (payload: T) => void;

function onEvent<T>(
  event: string,
  handler: EventHandler<T>,
): Promise<UnlistenFn> {
  return listen<T>(event, ({ payload }) => handler(payload));
}

function isUnavailableCommand(error: unknown, command: string): boolean {
  const message = error instanceof Error ? error.message : String(error);
  const normalized = message.toLowerCase();
  return (
    (normalized.includes(command) &&
      (normalized.includes("not found") || normalized.includes("unknown"))) ||
    normalized.includes("__tauri_internals__") ||
    normalized.includes("reading 'invoke'") ||
    normalized.includes('reading "invoke"')
  );
}

async function deleteAllLocalDataIfAvailable(): Promise<boolean> {
  const command = "delete_all_local_data";
  try {
    await invoke<void>(command);
    return true;
  } catch (error) {
    if (isUnavailableCommand(error, command)) {
      return false;
    }
    throw error;
  }
}

export const backend = {
  deleteAllLocalDataIfAvailable,
  exportSupportBundle: () => invoke<string>("export_support_bundle"),
  grantAgentBrowserConsent: (threadId: string) =>
    invoke<void>("grant_agent_browser_consent", { threadId }),
  revokeAgentBrowserConsent: (threadId: string) =>
    invoke<void>("revoke_agent_browser_consent", { threadId }),
  getProxyStatus: () => invoke<ProxyStatus>("get_proxy_status"),
  configureProxy: (baseUrl: string, apiKey: string) =>
    invoke<ProxyStatus>("configure_proxy", { baseUrl, apiKey }),
  fetchModels: () => invoke<ProxyModel[]>("fetch_active_models"),
  getCodePolicyStatus: () => invoke<CodePolicyStatus>("get_code_policy_status"),

  getBrowserStatus: () => invoke<BrowserStatus>("browser_status"),
  startBrowser: () => invoke<BrowserStatus>("browser_start"),
  stopBrowser: () => invoke<BrowserStatus>("browser_stop"),
  navigateBrowser: (url: string) => invoke<void>("browser_navigate", { url }),
  clickBrowserSelector: (selector: string) =>
    invoke<void>("browser_click_selector", { selector }),
  clickBrowserAt: (x: number, y: number) =>
    invoke<void>("browser_click_at", { x, y }),
  typeInBrowser: (selector: string, text: string) =>
    invoke<void>("browser_type", { selector, text }),

  getDesktopStatus: () => invoke<DesktopStatus>("desktop_status"),
  enableDesktopWorkMode: () =>
    invoke<DesktopStatus>("enable_desktop_work_mode"),
  disableDesktopWorkMode: () =>
    invoke<DesktopStatus>("disable_desktop_work_mode"),
  clickDesktopAt: (x: number, y: number) =>
    invoke<void>("desktop_click", { x, y }),
  typeOnDesktop: (text: string) => invoke<void>("desktop_type", { text }),

  listA2aTasks: () => invoke<A2aTask[]>("list_a2a_tasks"),
  getA2aStatus: () => invoke<A2aStatus>("get_a2a_status"),
  configureA2aServer: (settings: A2aServerSettings) =>
    invoke<A2aStatus>("configure_a2a_server", { settings }),
  generateA2aToken: () =>
    invoke<A2aTokenProvisioning>("generate_a2a_token"),
  regenerateA2aToken: () =>
    invoke<A2aTokenProvisioning>("regenerate_a2a_token"),
  deleteA2aToken: () => invoke<A2aStatus>("delete_a2a_token"),
  delegateA2aTask: ({ prompt, model, contextId }: DelegateA2aTaskRequest) =>
    invoke<A2aTask>("delegate_a2a_task", {
      prompt,
      model: model ?? null,
      contextId: contextId ?? null,
    }),
  getA2aTask: (id: string) => invoke<A2aTask>("get_a2a_task", { id }),
  cancelA2aTask: (id: string) => invoke<A2aTask>("cancel_a2a_task", { id }),

  getCodeMemoryStatus: () => invoke<CodeMemoryStatus>("get_code_memory_status"),
  indexCodeMemory: (root: string) =>
    invoke<CodeMemoryIndexResult>("index_code_memory", { root }),
  searchCodeMemory: (request: CodeMemorySearchRequest) =>
    invoke<CodeMemorySearchResult[]>("search_code_memory", { request }),
  clearCodeMemory: () => invoke<CodeMemoryStatus>("clear_code_memory"),

  onProxyStatus: (handler: EventHandler<ProxyStatus>) =>
    onEvent("proxy-status", handler),
  onA2aStatus: (handler: EventHandler<A2aStatus>) =>
    onEvent("a2a-status", handler),
  onBrowserFrame: (handler: EventHandler<BrowserFrame>) =>
    onEvent("browser-frame", handler),
  onCodeMemoryStatus: (handler: EventHandler<CodeMemoryStatus>) =>
    onEvent("code-memory-status", handler),
};
