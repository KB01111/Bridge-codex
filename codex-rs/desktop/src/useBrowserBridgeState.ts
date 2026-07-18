import { useCallback, useReducer, useRef } from "react";

import { backend } from "./backend";
import { clearBrowserFrame } from "./browserFrameStore";
import {
  bridgeErrorMessage,
  truncateBridgeText,
  type BridgeHookContext,
} from "./bridgeHookContext";
import type { BrowserStatus } from "./types";

type BrowserState = { status: BrowserStatus | null };
type BrowserAction =
  | { type: "status"; status: BrowserStatus }
  | { type: "reset" };

function browserReducer(state: BrowserState, action: BrowserAction): BrowserState {
  switch (action.type) {
    case "status":
      return state.status === action.status ? state : { status: action.status };
    case "reset":
      return { status: null };
  }
}

export function useBrowserBridgeState(context: BridgeHookContext) {
  const [state, dispatch] = useReducer(browserReducer, { status: null });
  const operationRunning = useRef(false);

  const applyStatus = useCallback(
    (status: BrowserStatus) => {
      dispatch({ type: "status", status });
      context.setError(status.error ?? null);
    },
    [context],
  );

  const runOperation = useCallback(
    async <T>(label: string, operation: () => Promise<T>): Promise<T | null> => {
      if (operationRunning.current) {
        context.appendTrace("Another browser operation is already running", "error");
        return null;
      }
      operationRunning.current = true;
      context.setBusy(true);
      context.setError(null);
      try {
        return await operation();
      } catch (error) {
        const message = bridgeErrorMessage(error);
        context.setError(message);
        context.appendTrace(`${label} failed: ${message}`, "error");
        return null;
      } finally {
        operationRunning.current = false;
        context.setBusy(false);
      }
    },
    [context],
  );

  const refreshStatus = useCallback(async () => {
    const status = await runOperation("Browser status refresh", backend.getBrowserStatus);
    if (status) {
      applyStatus(status);
    }
  }, [applyStatus, runOperation]);

  const start = useCallback(async () => {
    context.appendTrace("Starting the isolated Chromium session");
    clearBrowserFrame();
    const status = await runOperation("Agent browser startup", backend.startBrowser);
    if (status) {
      applyStatus(status);
      context.appendTrace("Agent browser is ready", "success");
    }
  }, [applyStatus, context, runOperation]);

  const stop = useCallback(async () => {
    const status = await runOperation("Agent browser shutdown", backend.stopBrowser);
    if (status) {
      clearBrowserFrame();
      applyStatus(status);
      context.appendTrace("Agent browser stopped", "success");
    }
  }, [applyStatus, context, runOperation]);

  const navigate = useCallback(
    async (url: string) => {
      const address = url.trim();
      if (!address) {
        return;
      }
      context.appendTrace(
        `Navigating agent browser to ${truncateBridgeText(address, 512)}`,
      );
      const completed = await runOperation("Browser navigation", async () => {
        await backend.navigateBrowser(address);
        return true;
      });
      if (completed) {
        const status = await backend.getBrowserStatus().catch(() => null);
        if (status) {
          applyStatus(status);
        }
        context.appendTrace("Agent browser navigation completed", "success");
      }
    },
    [applyStatus, context, runOperation],
  );

  const clickAt = useCallback(
    async (x: number, y: number) => {
      if (!Number.isFinite(x) || !Number.isFinite(y)) {
        context.appendTrace("Browser click coordinates must be finite", "error");
        return;
      }
      const completed = await runOperation("Browser click", async () => {
        await backend.clickBrowserAt(x, y);
        return true;
      });
      if (completed) {
        context.appendTrace(`Browser click at ${Math.round(x)}, ${Math.round(y)}`);
      }
    },
    [context, runOperation],
  );

  const clickSelector = useCallback(
    async (selector: string) => {
      const target = selector.trim();
      if (!target) {
        context.appendTrace("Enter a browser selector before clicking", "error");
        return;
      }
      const completed = await runOperation("Browser selector click", async () => {
        await backend.clickBrowserSelector(target);
        return true;
      });
      if (completed) {
        context.appendTrace(
          `Clicked browser selector ${truncateBridgeText(target, 256)}`,
          "success",
        );
      }
    },
    [context, runOperation],
  );

  const type = useCallback(
    async (selector: string, text: string) => {
      const target = selector.trim();
      if (!target) {
        context.appendTrace("Enter a browser selector before typing", "error");
        return;
      }
      const completed = await runOperation("Browser typing", async () => {
        await backend.typeInBrowser(target, text);
        return true;
      });
      if (completed) {
        context.appendTrace(
          `Typed into browser selector ${truncateBridgeText(target, 256)}`,
          "success",
        );
      }
    },
    [context, runOperation],
  );

  const reset = useCallback(() => {
    clearBrowserFrame();
    dispatch({ type: "reset" });
  }, []);

  return {
    status: state.status,
    applyStatus,
    refreshStatus,
    start,
    stop,
    navigate,
    clickAt,
    clickSelector,
    type,
    reset,
  };
}
