import * as Dialog from "@radix-ui/react-dialog";

import type { LocalDataState } from "../localDataReducer";

export type LegacyDataMigrationDialogProps = {
  state: LocalDataState;
  onExport: () => void;
  onClear: () => void;
};

function formatBytes(byteLength: number): string {
  if (byteLength < 1_024) {
    return `${byteLength} bytes`;
  }
  return `${(byteLength / 1_024).toFixed(1)} KB`;
}

export function LegacyDataMigrationDialog({
  state,
  onExport,
  onClear,
}: LegacyDataMigrationDialogProps) {
  const summary = state.legacyData;
  return (
    <Dialog.Root open={summary !== null}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content
          className="legacy-data-dialog"
          onEscapeKeyDown={(event) => event.preventDefault()}
          onInteractOutside={(event) => event.preventDefault()}
        >
          <p className="eyebrow">Privacy update</p>
          <Dialog.Title>Export your saved conversations</Dialog.Title>
          <Dialog.Description>
            An older Bridge Codex version kept conversation text in browser
            storage. Bridge no longer saves conversation content there. Export
            one archive, then remove the old browser copy to continue.
          </Dialog.Description>
          {summary && (
            <dl className="legacy-data-summary">
              <div>
                <dt>Conversations</dt>
                <dd>{summary.conversationCount || "Saved data found"}</dd>
              </div>
              <div>
                <dt>Messages</dt>
                <dd>{summary.messageCount || "Not available"}</dd>
              </div>
              <div>
                <dt>Archive size</dt>
                <dd>{formatBytes(summary.byteLength)}</dd>
              </div>
            </dl>
          )}
          {state.legacyExported && (
            <p className="data-success" role="status">
              Archive downloaded. You can now remove the browser copy.
            </p>
          )}
          {state.error && <p role="alert">{state.error}</p>}
          <div className="dialog-actions legacy-data-actions">
            <button type="button" onClick={onExport}>
              {state.legacyExported
                ? "Download archive again"
                : "Download conversation archive"}
            </button>
            <button
              type="button"
              disabled={!state.legacyExported}
              onClick={onClear}
            >
              Remove saved browser copy
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
