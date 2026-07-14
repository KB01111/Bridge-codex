import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  A2aStatus,
  A2aTask,
  BrowserFrame,
  BrowserStatus,
  ChatChunk,
  ChatRequest,
  CodePolicyStatus,
  CodeValidation,
  CodeMemoryIndexResult,
  CodeMemorySearchRequest,
  CodeMemorySearchResult,
  CodeMemoryStatus,
  DelegateA2aTaskRequest,
  DesktopStatus,
  LoginLaunch,
  ProxyModel,
  ProxyStatus,
  SandboxValidationEvent,
} from "./types";

type EventHandler<T> = (payload: T) => void;

function onEvent<T>(
  event: string,
  handler: EventHandler<T>,
): Promise<UnlistenFn> {
  return listen<T>(event, ({ payload }) => handler(payload));
}

export const backend = {
  getProxyStatus: () => invoke<ProxyStatus>("get_proxy_status"),
  ensureProxy: () => invoke<ProxyStatus>("ensure_cliproxyapi"),
  startLogin: () => invoke<LoginLaunch>("run_chatgpt_browser_login"),
  fetchModels: () => invoke<ProxyModel[]>("fetch_active_models"),
  startChat: (request: ChatRequest) =>
    invoke<string>("start_chat_completion", { request }),
  getCodePolicyStatus: () => invoke<CodePolicyStatus>("get_code_policy_status"),
  validateSandboxResponse: (response: string) =>
    invoke<CodeValidation>("validate_sandbox_response", { response }),

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
  onChatChunk: (handler: EventHandler<ChatChunk>) =>
    onEvent("chat-chunk", handler),
  onSandboxValidation: (handler: EventHandler<SandboxValidationEvent>) =>
    onEvent("sandbox-validation", handler),
  onBrowserFrame: (handler: EventHandler<BrowserFrame>) =>
    onEvent("browser-frame", handler),
  onCodeMemoryStatus: (handler: EventHandler<CodeMemoryStatus>) =>
    onEvent("code-memory-status", handler),
};
