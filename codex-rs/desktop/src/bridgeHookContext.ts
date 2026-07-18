import type { TraceEntry } from "./workbenchTypes";

export type BridgeHookContext = {
  appendTrace: (message: string, kind?: TraceEntry["kind"]) => void;
  setBusy: (busy: boolean) => void;
  setError: (error: string | null) => void;
};

export function bridgeErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  try {
    return JSON.stringify(error) || String(error);
  } catch {
    return String(error);
  }
}

export function truncateBridgeText(value: string, maxLength: number): string {
  if (value.length <= maxLength) {
    return value;
  }
  let end = Math.max(0, maxLength - 1);
  const finalCodeUnit = value.charCodeAt(end - 1);
  if (finalCodeUnit >= 0xd800 && finalCodeUnit <= 0xdbff) {
    end -= 1;
  }
  return `${value.slice(0, end)}…`;
}
