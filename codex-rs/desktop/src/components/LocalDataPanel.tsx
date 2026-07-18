import * as Dialog from "@radix-ui/react-dialog";
import { type Dispatch, useState } from "react";

import type {
  LocalDataAction,
  LocalDataState,
} from "../localDataReducer";

export type LocalDataPanelProps = {
  state: LocalDataState;
  dispatch: Dispatch<LocalDataAction>;
  conversationCount: number;
  currentMessageCount: number;
  busy: boolean;
  onExportSession: () => void;
  onArchiveCurrent: () => void;
  onDeleteAll: () => Promise<boolean>;
  onExportSupportBundle: () => Promise<string>;
};

function errorMessage(error: unknown): string {
  return error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : "Bridge could not delete the local data. Try again.";
}

export function LocalDataPanel({
  state,
  dispatch,
  conversationCount,
  currentMessageCount,
  busy,
  onExportSession,
  onArchiveCurrent,
  onDeleteAll,
  onExportSupportBundle,
}: LocalDataPanelProps) {
  const [supportBundlePath, setSupportBundlePath] = useState<string | null>(null);
  const [supportBundleError, setSupportBundleError] = useState<string | null>(null);
  async function deleteAll() {
    dispatch({ type: "delete_started" });
    try {
      const nativeCleanup = await onDeleteAll();
      dispatch({ type: "delete_succeeded", nativeCleanup });
    } catch (error) {
      dispatch({ type: "delete_failed", error: errorMessage(error) });
    }
  }

  return (
    <section className="local-data-panel" aria-labelledby="local-data-heading">
      <div className="section-heading-row">
        <div>
          <h3 id="local-data-heading">Local data</h3>
          <p>
            Codex app-server owns thread rollouts. Browser storage contains only
            the versioned interface preferences.
          </p>
        </div>
      </div>

      <div className="local-data-actions">
        <button
          type="button"
          disabled={conversationCount === 0}
          onClick={onExportSession}
        >
          Export session archive
        </button>
        <button
          type="button"
          disabled={busy || currentMessageCount === 0}
          onClick={onArchiveCurrent}
        >
          Archive current conversation
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => {
            setSupportBundlePath(null);
            setSupportBundleError(null);
            void onExportSupportBundle()
              .then(setSupportBundlePath)
              .catch((supportError: unknown) =>
                setSupportBundleError(errorMessage(supportError)),
              );
          }}
        >
          Export support bundle
        </button>
      </div>
      <p className="local-data-help">
        Archiving downloads the current thread, then asks app-server to archive
        its rollout.
      </p>
      {supportBundlePath && (
        <p className="support-bundle-path" role="status">
          Support bundle: <code>{supportBundlePath}</code>
        </p>
      )}
      {supportBundleError && <p role="alert">{supportBundleError}</p>}

      {state.notice && (
        <p className="data-success" role="status">
          {state.notice}
        </p>
      )}
      <div className="local-data-danger-zone">
        <div>
          <strong>Delete all local data</strong>
          <p>
            Stop local services and remove Codex threads, memory indexes,
            browser profiles, preferences, diagnostics, and legacy browser
            data owned by Bridge.
          </p>
        </div>
        <button
          type="button"
          disabled={busy}
          onClick={() => dispatch({ type: "delete_opened" })}
        >
          Delete all local data
        </button>
      </div>

      <Dialog.Root
        open={state.deleteOpen}
        onOpenChange={(open) =>
          dispatch({ type: open ? "delete_opened" : "delete_closed" })
        }
      >
        <Dialog.Portal>
          <Dialog.Overlay className="dialog-overlay" />
          <Dialog.Content className="destructive-confirm-dialog local-data-delete-dialog">
            <Dialog.Title>Delete all local Bridge data?</Dialog.Title>
            <Dialog.Description>
              This stops local services and permanently removes Bridge-owned
              Codex threads, memory indexes, browser profiles, preferences,
              diagnostics, and legacy browser data. This cannot be undone.
            </Dialog.Description>
            <label htmlFor="delete-local-data-confirmation">
              Type <strong>DELETE ALL</strong> to confirm
            </label>
            <input
              id="delete-local-data-confirmation"
              value={state.deleteConfirmation}
              autoComplete="off"
              disabled={state.deleting}
              onChange={(event) =>
                dispatch({
                  type: "delete_confirmation_changed",
                  value: event.currentTarget.value,
                })
              }
            />
            {state.error && <p role="alert">{state.error}</p>}
            <div className="dialog-actions">
              <button
                type="button"
                disabled={state.deleting}
                onClick={() => dispatch({ type: "delete_closed" })}
              >
                Keep local data
              </button>
              <button
                type="button"
                disabled={
                  state.deleting || state.deleteConfirmation !== "DELETE ALL"
                }
                onClick={() => void deleteAll()}
              >
                {state.deleting ? "Deleting…" : "Delete all local data"}
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </section>
  );
}
