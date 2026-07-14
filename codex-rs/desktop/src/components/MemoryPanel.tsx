import * as Dialog from "@radix-ui/react-dialog";
import { type FormEvent, useEffect, useRef, useState } from "react";

import type {
  CodeMemorySearchRequest,
  CodeMemorySearchResult,
  CodeMemoryStatus,
} from "../types";

export type MemoryPanelProps = {
  status: CodeMemoryStatus | null;
  results: CodeMemorySearchResult[];
  warnings: string[];
  busy: boolean;
  error?: string | null;
  onRefresh: () => Promise<void>;
  onIndex: (root: string) => Promise<void>;
  onSearch: (request: CodeMemorySearchRequest) => Promise<void>;
  onClear: () => Promise<void>;
  onClearResults: () => void;
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

export function MemoryPanel({
  status,
  results,
  warnings,
  busy,
  error,
  onRefresh,
  onIndex,
  onSearch,
  onClear,
  onClearResults,
}: MemoryPanelProps) {
  const [root, setRoot] = useState(status?.root ?? "");
  const [query, setQuery] = useState("");
  const [maxResults, setMaxResults] = useState(12);
  const [graphWeight, setGraphWeight] = useState(0.25);
  const [clearOpen, setClearOpen] = useState(false);
  const hydratedRoot = useRef(false);

  useEffect(() => {
    if (!hydratedRoot.current && status?.root) {
      hydratedRoot.current = true;
      setRoot(status.root);
    }
  }, [status?.root]);

  function submitIndex(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const path = root.trim();
    if (path) {
      void onIndex(path);
    }
  }

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const searchQuery = query.trim();
    if (searchQuery) {
      void onSearch({ query: searchQuery, maxResults, graphWeight });
    }
  }

  async function clearMemory() {
    await onClear();
    setClearOpen(false);
  }

  return (
    <div className="memory-panel" aria-busy={busy}>
      <section aria-labelledby="memory-status-heading">
        <div className="section-heading-row">
          <div>
            <h3 id="memory-status-heading">Local code memory</h3>
            <p>
              Tree-sitter chunks, BM25 retrieval, and a bounded symbol/import
              graph.
            </p>
          </div>
          <button
            type="button"
            disabled={busy}
            onClick={() => void onRefresh()}
          >
            Refresh status
          </button>
        </div>

        {status === null ? (
          <p role="status">Checking the local index…</p>
        ) : (
          <>
            <dl className="memory-status-grid">
              <div>
                <dt>Status</dt>
                <dd>
                  {status.indexing
                    ? "Indexing"
                    : status.ready
                      ? "Ready"
                      : "Not indexed"}
                </dd>
              </div>
              <div>
                <dt>Source root</dt>
                <dd title={status.root ?? undefined}>
                  {status.root ?? "None"}
                </dd>
              </div>
              <div>
                <dt>Indexed</dt>
                <dd>
                  {status.indexedAt
                    ? new Date(status.indexedAt).toLocaleString()
                    : "Never"}
                </dd>
              </div>
              <div>
                <dt>Files</dt>
                <dd>{status.statistics.indexedFiles}</dd>
              </div>
              <div>
                <dt>Chunks</dt>
                <dd>{status.statistics.chunks}</dd>
              </div>
              <div>
                <dt>Symbols</dt>
                <dd>{status.statistics.symbols}</dd>
              </div>
              <div>
                <dt>Graph edges</dt>
                <dd>{status.statistics.graphEdges}</dd>
              </div>
              <div>
                <dt>Source size</dt>
                <dd>{formatBytes(status.statistics.sourceBytes)}</dd>
              </div>
              <div>
                <dt>Parser</dt>
                <dd>{status.parser}</dd>
              </div>
              <div>
                <dt>Storage</dt>
                <dd>{status.storage}</dd>
              </div>
              <div>
                <dt>Network</dt>
                <dd>{status.networkAccess}</dd>
              </div>
            </dl>
            {status.error && <p role="alert">{status.error}</p>}
          </>
        )}
      </section>

      <section aria-labelledby="memory-index-heading">
        <h3 id="memory-index-heading">Index a source directory</h3>
        <form className="memory-index-form" onSubmit={submitIndex}>
          <label htmlFor="memory-root">Absolute directory path</label>
          <input
            id="memory-root"
            value={root}
            onChange={(event) => setRoot(event.currentTarget.value)}
            placeholder="C:\\projects\\my-app or /sandbox/project"
            spellCheck={false}
            autoComplete="off"
            disabled={busy}
          />
          <button type="submit" disabled={busy || !root.trim()}>
            {status?.indexing || busy
              ? "Indexing…"
              : status?.ready
                ? "Reindex"
                : "Create index"}
          </button>
        </form>
        <p>
          Indexing is offline and bounded. Vendor, build, VCS, binary, symlink,
          and oversized files are skipped.
        </p>
        {warnings.length > 0 && (
          <details className="memory-warnings">
            <summary>
              {warnings.length} indexing warning
              {warnings.length === 1 ? "" : "s"}
            </summary>
            <ul>
              {warnings.map((warning, index) => (
                <li key={`${warning}-${index}`}>{warning}</li>
              ))}
            </ul>
          </details>
        )}
      </section>

      {error && <p role="alert">{error}</p>}

      <section aria-labelledby="memory-search-heading">
        <div className="section-heading-row">
          <h3 id="memory-search-heading">Search code memory</h3>
          <button
            type="button"
            disabled={results.length === 0}
            onClick={onClearResults}
          >
            Clear results
          </button>
        </div>
        <form className="memory-search-form" onSubmit={submitSearch}>
          <label htmlFor="memory-query">Code, symbol, or concept</label>
          <input
            id="memory-query"
            type="search"
            value={query}
            onChange={(event) => setQuery(event.currentTarget.value)}
            placeholder="Where is authentication initialized?"
            disabled={!status?.ready || busy}
          />
          <label htmlFor="memory-max-results">Maximum results</label>
          <input
            id="memory-max-results"
            type="number"
            min={1}
            max={100}
            step={1}
            value={maxResults}
            onChange={(event) => {
              const value = event.currentTarget.valueAsNumber;
              if (Number.isFinite(value)) {
                setMaxResults(value);
              }
            }}
            disabled={!status?.ready || busy}
          />
          <label htmlFor="memory-graph-weight">Graph weight</label>
          <input
            id="memory-graph-weight"
            type="number"
            min={0}
            max={2}
            step={0.05}
            value={graphWeight}
            onChange={(event) => {
              const value = event.currentTarget.valueAsNumber;
              if (Number.isFinite(value)) {
                setGraphWeight(value);
              }
            }}
            disabled={!status?.ready || busy}
          />
          <button
            type="submit"
            disabled={!status?.ready || busy || !query.trim()}
          >
            {busy ? "Searching…" : "Search memory"}
          </button>
        </form>

        {!status?.ready ? (
          <p className="empty-state">Index a directory before searching.</p>
        ) : results.length === 0 ? (
          <p className="empty-state">
            Run a search to see ranked structural code chunks.
          </p>
        ) : (
          <ol className="memory-results">
            {results.map((result) => (
              <li key={result.chunk.id}>
                <article>
                  <header>
                    <div>
                      <strong>
                        {result.chunk.symbol ?? result.chunk.kind}
                      </strong>
                      <span>{result.chunk.language}</span>
                    </div>
                    <span aria-label={`Score ${result.score.toFixed(3)}`}>
                      {result.score.toFixed(3)}
                    </span>
                  </header>
                  <p title={result.chunk.path}>
                    {result.chunk.path}:{result.chunk.startLine}
                    {result.chunk.endLine !== result.chunk.startLine
                      ? `–${result.chunk.endLine}`
                      : ""}
                  </p>
                  <pre>{result.chunk.source}</pre>
                  <details>
                    <summary>Ranking detail</summary>
                    <dl>
                      <div>
                        <dt>Lexical</dt>
                        <dd>{result.lexicalScore.toFixed(3)}</dd>
                      </div>
                      <div>
                        <dt>Graph</dt>
                        <dd>{result.graphScore.toFixed(3)}</dd>
                      </div>
                    </dl>
                    {result.explanations.length > 0 && (
                      <ul>
                        {result.explanations.map((explanation, index) => (
                          <li
                            key={`${explanation.fromChunkId}-${explanation.relation}-${index}`}
                          >
                            {explanation.relation} from{" "}
                            {explanation.fromChunkId} (+
                            {explanation.contribution.toFixed(3)})
                          </li>
                        ))}
                      </ul>
                    )}
                  </details>
                </article>
              </li>
            ))}
          </ol>
        )}
      </section>

      <section
        className="memory-danger-zone"
        aria-labelledby="memory-clear-heading"
      >
        <h3 id="memory-clear-heading">Stored index</h3>
        <p>
          Clearing removes the local JSON snapshot and all in-memory search
          data.
        </p>
        <Dialog.Root open={clearOpen} onOpenChange={setClearOpen}>
          <Dialog.Trigger asChild>
            <button type="button" disabled={!status?.ready || busy}>
              Clear code memory
            </button>
          </Dialog.Trigger>
          <Dialog.Portal>
            <Dialog.Overlay className="dialog-overlay" />
            <Dialog.Content className="memory-clear-dialog">
              <Dialog.Title>Clear the local code index?</Dialog.Title>
              <Dialog.Description>
                This removes the saved snapshot. Source files are never
                modified.
              </Dialog.Description>
              <div className="dialog-actions">
                <Dialog.Close asChild>
                  <button type="button">Keep index</button>
                </Dialog.Close>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void clearMemory()}
                >
                  Clear index
                </button>
              </div>
            </Dialog.Content>
          </Dialog.Portal>
        </Dialog.Root>
      </section>
    </div>
  );
}
