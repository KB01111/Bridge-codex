import { useCallback, useReducer, useRef } from "react";

import { backend } from "./backend";
import {
  bridgeErrorMessage,
  type BridgeHookContext,
} from "./bridgeHookContext";
import type { CodeMemorySearchResult, CodeMemoryStatus } from "./types";

type CodeMemoryState = {
  status: CodeMemoryStatus | null;
  results: CodeMemorySearchResult[];
  warnings: string[];
};

type CodeMemoryAction =
  | { type: "status"; status: CodeMemoryStatus }
  | { type: "indexing" }
  | {
      type: "indexed";
      status: CodeMemoryStatus;
      warnings: string[];
    }
  | { type: "results"; results: CodeMemorySearchResult[] }
  | { type: "cleared"; status: CodeMemoryStatus }
  | { type: "reset" };

const initialState: CodeMemoryState = {
  status: null,
  results: [],
  warnings: [],
};

function codeMemoryReducer(
  state: CodeMemoryState,
  action: CodeMemoryAction,
): CodeMemoryState {
  switch (action.type) {
    case "status":
      return { ...state, status: action.status };
    case "indexing":
      return {
        ...state,
        status: state.status
          ? { ...state.status, indexing: true, error: null }
          : null,
      };
    case "indexed":
      return {
        status: action.status,
        warnings: action.warnings.slice(0, 256),
        results: [],
      };
    case "results":
      return { ...state, results: action.results };
    case "cleared":
      return { status: action.status, results: [], warnings: [] };
    case "reset":
      return initialState;
  }
}

export function useCodeMemoryBridgeState(context: BridgeHookContext) {
  const [state, dispatch] = useReducer(codeMemoryReducer, initialState);
  const operationRunning = useRef(false);

  const applyStatus = useCallback(
    (status: CodeMemoryStatus) => {
      dispatch({ type: "status", status });
      context.setError(status.error ?? null);
    },
    [context],
  );

  const runOperation = useCallback(
    async <T>(label: string, operation: () => Promise<T>): Promise<T | null> => {
      if (operationRunning.current) {
        context.appendTrace(
          "Another code-memory operation is already running",
          "error",
        );
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
    const status = await runOperation(
      "Code-memory status refresh",
      backend.getCodeMemoryStatus,
    );
    if (status) {
      applyStatus(status);
    }
  }, [applyStatus, runOperation]);

  const index = useCallback(
    async (root: string) => {
      const selectedRoot = root.trim();
      if (!selectedRoot) {
        context.appendTrace("Choose a source directory to index", "error");
        return false;
      }
      dispatch({ type: "indexing" });
      const result = await runOperation("Code-memory indexing", () =>
        backend.indexCodeMemory(selectedRoot),
      );
      if (!result) {
        const status = await backend.getCodeMemoryStatus().catch(() => null);
        if (status) {
          applyStatus(status);
        }
        return false;
      }
      dispatch({
        type: "indexed",
        status: result.status,
        warnings: result.warnings,
      });
      context.appendTrace(
        `Indexed ${result.status.statistics.indexedFiles} source file${result.status.statistics.indexedFiles === 1 ? "" : "s"} into ${result.status.statistics.chunks} structural chunks`,
        "success",
      );
      return true;
    },
    [applyStatus, context, runOperation],
  );

  const search = useCallback(
    async (
      query: string,
      options: { maxResults?: number; graphWeight?: number } = {},
    ) => {
      const searchQuery = query.trim();
      if (!searchQuery) {
        dispatch({ type: "results", results: [] });
        return [];
      }
      const maxResults = Math.min(
        100,
        Math.max(1, Math.round(options.maxResults ?? 12)),
      );
      const graphWeight = Math.min(2, Math.max(0, options.graphWeight ?? 0.25));
      const results = await runOperation("Code-memory search", () =>
        backend.searchCodeMemory({
          query: searchQuery,
          maxResults,
          graphWeight,
        }),
      );
      if (!results) {
        return [];
      }
      dispatch({ type: "results", results });
      context.appendTrace(
        `Code memory returned ${results.length} result${results.length === 1 ? "" : "s"}`,
        "success",
      );
      return results;
    },
    [context, runOperation],
  );

  const clear = useCallback(async () => {
    const status = await runOperation("Code-memory cleanup", backend.clearCodeMemory);
    if (status) {
      dispatch({ type: "cleared", status });
      context.appendTrace("Code memory cleared", "success");
    }
  }, [context, runOperation]);

  const clearResults = useCallback(
    () => dispatch({ type: "results", results: [] }),
    [],
  );
  const reset = useCallback(() => dispatch({ type: "reset" }), []);

  return {
    ...state,
    applyStatus,
    refreshStatus,
    index,
    search,
    clear,
    clearResults,
    reset,
  };
}
