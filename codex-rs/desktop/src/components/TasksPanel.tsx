import { type FormEvent, useEffect, useState } from "react";

import type {
  A2aStatus,
  A2aTask,
  A2aTaskState,
  DelegateA2aTaskRequest,
  ProxyModel,
} from "../types";

export type TasksPanelProps = {
  status: A2aStatus | null;
  tasks: A2aTask[];
  selectedTask: A2aTask | null;
  models: ProxyModel[];
  defaultModel: string;
  busy: boolean;
  error?: string | null;
  onRefresh: () => Promise<void>;
  onSelectTask: (id: string | null) => void;
  onDelegate: (request: DelegateA2aTaskRequest) => Promise<void>;
  onCancel: (id: string) => Promise<void>;
};

function taskStateLabel(state: A2aTaskState): string {
  switch (state) {
    case "TASK_STATE_WORKING":
      return "Working";
    case "TASK_STATE_COMPLETED":
      return "Completed";
    case "TASK_STATE_FAILED":
      return "Failed";
    case "TASK_STATE_CANCELED":
      return "Canceled";
  }
}

function messageText(task: A2aTask): string | null {
  const text = task.status.message?.parts
    .map((part) => part.text)
    .join("\n")
    .trim();
  return text || null;
}

export function TasksPanel({
  status,
  tasks,
  selectedTask,
  models,
  defaultModel,
  busy,
  error,
  onRefresh,
  onSelectTask,
  onDelegate,
  onCancel,
}: TasksPanelProps) {
  const [prompt, setPrompt] = useState("");
  const [model, setModel] = useState(defaultModel);
  const [contextId, setContextId] = useState("");

  useEffect(() => {
    if (!model && defaultModel) {
      setModel(defaultModel);
    }
  }, [defaultModel, model]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const content = prompt.trim();
    if (!content || busy) {
      return;
    }
    await onDelegate({
      prompt: content,
      model: model || null,
      contextId: contextId.trim() || null,
    });
  }

  function selectTask(task: A2aTask) {
    onSelectTask(task.id);
  }

  return (
    <div className="tasks-panel" aria-busy={busy}>
      <section aria-labelledby="delegate-heading">
        <div className="section-heading-row">
          <div>
            <h3 id="delegate-heading">Delegate a task</h3>
            <p>Queues work through the local A2A HTTP+JSON endpoint.</p>
          </div>
          <span data-state={status?.running ? "online" : "offline"}>
            {status === null
              ? "Checking…"
              : status.running
                ? "Listening"
                : "Offline"}
          </span>
        </div>
        <p>{status?.address ?? "127.0.0.1:8120"}</p>
        {status?.error && <p role="alert">{status.error}</p>}

        <form className="delegate-form" onSubmit={submit}>
          <label htmlFor="a2a-prompt">Task</label>
          <textarea
            id="a2a-prompt"
            rows={4}
            value={prompt}
            onChange={(event) => setPrompt(event.currentTarget.value)}
            placeholder="Describe the work to delegate"
            disabled={!status?.running || busy}
            required
          />

          <label htmlFor="a2a-model">Model</label>
          <select
            id="a2a-model"
            value={model}
            onChange={(event) => setModel(event.currentTarget.value)}
            disabled={!status?.running || busy}
          >
            <option value="">Use active/default model</option>
            {models.map((item) => (
              <option key={item.id} value={item.id}>
                {item.id}
              </option>
            ))}
          </select>

          <label htmlFor="a2a-context">Context ID (optional)</label>
          <input
            id="a2a-context"
            value={contextId}
            onChange={(event) => setContextId(event.currentTarget.value)}
            placeholder="Continue an existing task context"
            disabled={!status?.running || busy}
            spellCheck={false}
          />

          <button
            type="submit"
            disabled={!status?.running || busy || !prompt.trim()}
          >
            {busy ? "Delegating…" : "Delegate task"}
          </button>
        </form>
      </section>

      {error && <p role="alert">{error}</p>}

      <section aria-labelledby="task-list-heading">
        <div className="section-heading-row">
          <div>
            <h3 id="task-list-heading">Local tasks</h3>
            <span>{tasks.length} retained</span>
          </div>
          <button
            type="button"
            disabled={busy}
            onClick={() => void onRefresh()}
          >
            Refresh tasks
          </button>
        </div>

        {tasks.length === 0 ? (
          <p className="empty-state">No A2A tasks have been delegated yet.</p>
        ) : (
          <ul className="task-list">
            {tasks.map((task) => (
              <li key={task.id}>
                <button
                  type="button"
                  aria-pressed={selectedTask?.id === task.id}
                  onClick={() => selectTask(task)}
                >
                  <span>{taskStateLabel(task.status.state)}</span>
                  <strong>{messageText(task) ?? task.id}</strong>
                  <time dateTime={task.status.timestamp}>
                    {new Date(task.status.timestamp).toLocaleString()}
                  </time>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="task-detail-heading">
        <h3 id="task-detail-heading">Task detail</h3>
        {!selectedTask ? (
          <p className="empty-state">
            Select a task to inspect its status and artifacts.
          </p>
        ) : (
          <article className="task-detail">
            <header>
              <div>
                <span>{taskStateLabel(selectedTask.status.state)}</span>
                <h4>{selectedTask.id}</h4>
              </div>
              {selectedTask.status.state === "TASK_STATE_WORKING" && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void onCancel(selectedTask.id)}
                >
                  Cancel task
                </button>
              )}
            </header>
            <dl>
              <div>
                <dt>Context</dt>
                <dd>{selectedTask.contextId}</dd>
              </div>
              <div>
                <dt>Updated</dt>
                <dd>
                  <time dateTime={selectedTask.status.timestamp}>
                    {new Date(selectedTask.status.timestamp).toLocaleString()}
                  </time>
                </dd>
              </div>
            </dl>
            {messageText(selectedTask) && (
              <pre>{messageText(selectedTask)}</pre>
            )}
            {selectedTask.artifacts.length === 0 ? (
              <p className="empty-state">No artifacts have been produced.</p>
            ) : (
              <div className="task-artifacts">
                <h5>Artifacts</h5>
                {selectedTask.artifacts.map((artifact) => (
                  <article key={artifact.artifactId}>
                    <h6>{artifact.name}</h6>
                    <pre>
                      {artifact.parts.map((part) => part.text).join("\n")}
                    </pre>
                  </article>
                ))}
              </div>
            )}
          </article>
        )}
      </section>
    </div>
  );
}
