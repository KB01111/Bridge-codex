import { useCallback, useEffect, useReducer, useRef } from "react";

import { backend } from "./backend";
import {
  bridgeErrorMessage,
  type BridgeHookContext,
} from "./bridgeHookContext";
import type {
  A2aStatus,
  A2aTask,
  DelegateA2aTaskRequest,
} from "./types";

type A2aState = {
  status: A2aStatus | null;
  tasks: A2aTask[];
  selectedTask: A2aTask | null;
};

type A2aAction =
  | { type: "status"; status: A2aStatus }
  | { type: "tasks"; tasks: A2aTask[] }
  | { type: "task"; task: A2aTask; select: boolean }
  | { type: "select"; task: A2aTask | null }
  | { type: "reset" };

const initialState: A2aState = {
  status: null,
  tasks: [],
  selectedTask: null,
};

function replaceTask(tasks: A2aTask[], task: A2aTask): A2aTask[] {
  return tasks.some((candidate) => candidate.id === task.id)
    ? tasks.map((candidate) => (candidate.id === task.id ? task : candidate))
    : [task, ...tasks];
}

function a2aReducer(state: A2aState, action: A2aAction): A2aState {
  switch (action.type) {
    case "status":
      return { ...state, status: action.status };
    case "tasks":
      return {
        ...state,
        tasks: action.tasks,
        selectedTask: state.selectedTask
          ? (action.tasks.find((task) => task.id === state.selectedTask?.id) ??
            null)
          : null,
      };
    case "task":
      return {
        ...state,
        tasks: replaceTask(state.tasks, action.task),
        selectedTask: action.select
          ? action.task
          : state.selectedTask?.id === action.task.id
            ? action.task
            : state.selectedTask,
      };
    case "select":
      return { ...state, selectedTask: action.task };
    case "reset":
      return initialState;
  }
}

let taskListRequest: Promise<A2aTask[]> | null = null;

function fetchTasksShared(): Promise<A2aTask[]> {
  if (!taskListRequest) {
    const request = backend.listA2aTasks();
    taskListRequest = request;
    const clear = () => {
      if (taskListRequest === request) {
        taskListRequest = null;
      }
    };
    void request.then(clear, clear);
  }
  return taskListRequest;
}

export function useA2aBridgeState(
  context: BridgeHookContext,
  selectedModel: string,
) {
  const [state, dispatch] = useReducer(a2aReducer, initialState);
  const stateRef = useRef(state);
  const operationRunning = useRef(false);
  const mutationGeneration = useRef(0);
  stateRef.current = state;

  const applyStatus = useCallback(
    (status: A2aStatus) => {
      dispatch({ type: "status", status });
      context.setError(status.error ?? null);
    },
    [context],
  );

  const runOperation = useCallback(
    async <T>(label: string, operation: () => Promise<T>): Promise<T | null> => {
      if (operationRunning.current) {
        context.appendTrace("Another A2A operation is already running", "error");
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

  const refreshTasks = useCallback(
    async (silent = false) => {
      if (operationRunning.current) {
        return;
      }
      const generation = mutationGeneration.current;
      if (!silent) {
        context.setBusy(true);
      }
      try {
        const tasks = await fetchTasksShared();
        if (generation === mutationGeneration.current) {
          dispatch({ type: "tasks", tasks });
          context.setError(null);
        }
      } catch (error) {
        context.setError(bridgeErrorMessage(error));
      } finally {
        if (!silent) {
          context.setBusy(false);
        }
      }
    },
    [context],
  );

  useEffect(() => {
    let disposed = false;
    let timeout: number | undefined;
    const poll = async () => {
      if (!document.hidden) {
        await refreshTasks(true);
      }
      if (!disposed) {
        const working = stateRef.current.tasks.some(
          (task) => task.status.state === "TASK_STATE_WORKING",
        );
        timeout = window.setTimeout(() => void poll(), working ? 1_500 : 4_000);
      }
    };
    void poll();
    return () => {
      disposed = true;
      if (timeout !== undefined) {
        window.clearTimeout(timeout);
      }
    };
  }, [refreshTasks]);

  const getTask = useCallback(
    async (id: string) => {
      const task = await runOperation("A2A task refresh", () =>
        backend.getA2aTask(id),
      );
      if (task) {
        dispatch({ type: "task", task, select: true });
      }
      return task;
    },
    [runOperation],
  );

  const selectTask = useCallback(
    async (id: string | null) => {
      if (!id) {
        dispatch({ type: "select", task: null });
        return null;
      }
      const cached = stateRef.current.tasks.find((task) => task.id === id);
      if (cached) {
        dispatch({ type: "select", task: cached });
      }
      return getTask(id);
    },
    [getTask],
  );

  const configureServer = useCallback(
    async (enabled: boolean, port: number) => {
      const status = await runOperation("A2A configuration", () =>
        backend.configureA2aServer({ enabled, port }),
      );
      if (status) {
        applyStatus(status);
        context.appendTrace(
          enabled ? `A2A enabled on loopback port ${port}` : "A2A disabled",
          "success",
        );
      }
      return status;
    },
    [applyStatus, context, runOperation],
  );

  const provisionToken = useCallback(
    async (regenerate: boolean) => {
      const result = await runOperation(
        regenerate ? "A2A token regeneration" : "A2A token generation",
        regenerate ? backend.regenerateA2aToken : backend.generateA2aToken,
      );
      if (!result) {
        return null;
      }
      applyStatus(result.status);
      context.appendTrace(
        regenerate ? "A2A bearer token replaced" : "A2A bearer token created",
        "success",
      );
      return result.token;
    },
    [applyStatus, context, runOperation],
  );

  const deleteToken = useCallback(async () => {
    const status = await runOperation("A2A token deletion", backend.deleteA2aToken);
    if (status) {
      applyStatus(status);
      context.appendTrace("A2A bearer token deleted", "success");
    }
    return status;
  }, [applyStatus, context, runOperation]);

  const delegateTask = useCallback(
    async (request: DelegateA2aTaskRequest) => {
      const prompt = request.prompt.trim();
      if (!prompt) {
        context.appendTrace("Enter an A2A task before delegating", "error");
        return null;
      }
      mutationGeneration.current += 1;
      const task = await runOperation("A2A delegation", () =>
        backend.delegateA2aTask({
          ...request,
          prompt,
          model: request.model || selectedModel || null,
        }),
      );
      if (task) {
        dispatch({ type: "task", task, select: true });
        context.appendTrace(`Delegated A2A task ${task.id}`, "success");
      }
      return task;
    },
    [context, runOperation, selectedModel],
  );

  const cancelTask = useCallback(
    async (id: string) => {
      mutationGeneration.current += 1;
      const task = await runOperation("A2A cancellation", () =>
        backend.cancelA2aTask(id),
      );
      if (task) {
        dispatch({ type: "task", task, select: false });
        context.appendTrace(`Canceled A2A task ${id}`, "success");
      }
      return task;
    },
    [context, runOperation],
  );

  const reset = useCallback(() => dispatch({ type: "reset" }), []);

  return {
    ...state,
    applyStatus,
    refreshTasks,
    getTask,
    selectTask,
    configureServer,
    provisionToken,
    deleteToken,
    delegateTask,
    cancelTask,
    reset,
  };
}
