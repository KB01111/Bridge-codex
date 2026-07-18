import { useState } from "react";

import type { RequestId } from "../../../app-server-protocol/schema/typescript/RequestId";
import type { ServerRequest } from "../../../app-server-protocol/schema/typescript/ServerRequest";
import type { PendingServerRequest } from "../agentRuntimeReducer";

export type ServerRequestQueueProps = {
  requests: PendingServerRequest[];
  onResolve: (requestId: RequestId, result: unknown) => Promise<void>;
  onDeny: (requestId: RequestId) => Promise<void>;
  onExecuteDynamicTool: (
    request: Extract<ServerRequest, { method: "item/tool/call" }>,
  ) => Promise<boolean>;
};

type CommandApprovalRequest = Extract<
  ServerRequest,
  { method: "item/commandExecution/requestApproval" }
>;

type CommandApprovalParams = CommandApprovalRequest["params"] & {
  additionalPermissions?: unknown;
  availableDecisions?: unknown;
};

function pretty(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function requestLabel(request: ServerRequest): string {
  switch (request.method) {
    case "item/commandExecution/requestApproval":
      return "Command approval";
    case "item/fileChange/requestApproval":
      return "File-change approval";
    case "item/permissions/requestApproval":
      return "Additional permissions";
    case "item/tool/requestUserInput":
      return "Agent question";
    case "mcpServer/elicitation/request":
      return `MCP request · ${request.params.serverName}`;
    case "item/tool/call":
      if (request.params.tool === "memory_search") {
        return "Search code memory";
      }
      if (request.params.tool === "browser_observe") {
        return "Allow browser observation";
      }
      if (request.params.tool.startsWith("browser_")) {
        return "Approve browser state change";
      }
      return `Dynamic tool · ${request.params.tool}`;
    case "account/chatgptAuthTokens/refresh":
    case "attestation/generate":
    case "applyPatchApproval":
    case "execCommandApproval":
      return "Unsupported request denied";
  }
}

function CommandApprovalRequestCard({
  request,
  pending,
  onResolve,
  onDeny,
}: {
  request: CommandApprovalRequest;
  pending: PendingServerRequest;
  onResolve: ServerRequestQueueProps["onResolve"];
  onDeny: ServerRequestQueueProps["onDeny"];
}) {
  const params = request.params as CommandApprovalParams;
  const availableDecisions = Array.isArray(params.availableDecisions)
    ? params.availableDecisions
    : [];
  const canAccept = availableDecisions.some(
    (decision) => decision === "accept",
  );
  const denyDecision = availableDecisions.some(
    (decision) => decision === "decline",
  )
    ? "decline"
    : availableDecisions.some((decision) => decision === "cancel")
      ? "cancel"
      : null;

  return (
    <>
      <p>{params.reason ?? "The agent wants to run a command."}</p>
      <dl className="approval-scope">
        {params.command && (
          <div>
            <dt>Command</dt>
            <dd>
              <pre>{params.command}</pre>
            </dd>
          </div>
        )}
        {params.cwd && (
          <div>
            <dt>Working directory</dt>
            <dd>{params.cwd}</dd>
          </div>
        )}
        {params.environmentId && (
          <div>
            <dt>Execution environment</dt>
            <dd>{params.environmentId}</dd>
          </div>
        )}
        {params.networkApprovalContext && (
          <div>
            <dt>Network destination</dt>
            <dd>
              {params.networkApprovalContext.protocol}://
              {params.networkApprovalContext.host}
            </dd>
          </div>
        )}
        {params.commandActions && params.commandActions.length > 0 && (
          <div>
            <dt>Parsed command actions</dt>
            <dd>
              <pre>{pretty(params.commandActions)}</pre>
            </dd>
          </div>
        )}
        {params.additionalPermissions != null && (
          <div>
            <dt>Additional filesystem or network permissions</dt>
            <dd>
              <pre>{pretty(params.additionalPermissions)}</pre>
            </dd>
          </div>
        )}
        {params.proposedExecpolicyAmendment && (
          <div>
            <dt>Proposed persistent command-policy amendment</dt>
            <dd>
              <pre>{pretty(params.proposedExecpolicyAmendment)}</pre>
            </dd>
          </div>
        )}
        {params.proposedNetworkPolicyAmendments &&
          params.proposedNetworkPolicyAmendments.length > 0 && (
            <div>
              <dt>Proposed persistent network-policy amendments</dt>
              <dd>
                <pre>{pretty(params.proposedNetworkPolicyAmendments)}</pre>
              </dd>
            </div>
          )}
      </dl>
      {!canAccept && (
        <p role="status">
          This request does not advertise a supported one-time approval. Bridge
          can only deny it.
        </p>
      )}
      <div className="server-request-actions">
        <button
          type="button"
          disabled={pending.resolving}
          onClick={() => {
            if (denyDecision) {
              void onResolve(request.id, { decision: denyDecision });
            } else {
              void onDeny(request.id);
            }
          }}
        >
          {denyDecision === "cancel" ? "Deny and stop turn" : "Deny"}
        </button>
        {canAccept && (
          <button
            type="button"
            disabled={pending.resolving}
            onClick={() =>
              void onResolve(request.id, { decision: "accept" })
            }
          >
            Allow once
          </button>
        )}
      </div>
    </>
  );
}

function DynamicToolRequest({
  request,
  pending,
  browserConsent,
  onConsent,
  onExecute,
  onDeny,
}: {
  request: Extract<ServerRequest, { method: "item/tool/call" }>;
  pending: PendingServerRequest;
  browserConsent: boolean;
  onConsent: () => void;
  onExecute: ServerRequestQueueProps["onExecuteDynamicTool"];
  onDeny: ServerRequestQueueProps["onDeny"];
}) {
  const browserObserve = request.params.tool === "browser_observe";
  const browserMutation =
    request.params.tool.startsWith("browser_") && !browserObserve;
  const memorySearch = request.params.tool === "memory_search";
  return (
    <>
      <p>
        {memorySearch
          ? "The agent wants to search the local code-memory index with these arguments."
          : browserObserve
            ? browserConsent
              ? "Browser observation is allowed for this thread, but every call still requires confirmation."
              : "This grants browser-observation access for this thread. It does not approve browser state changes."
            : browserMutation
              ? "This action changes browser state and always requires explicit approval."
              : "The agent requested a client-provided dynamic tool."}
      </p>
      <pre>{pretty(request.params.arguments)}</pre>
      <div className="server-request-actions">
        <button
          type="button"
          disabled={pending.resolving}
          onClick={() => void onDeny(request.id)}
        >
          Deny
        </button>
        <button
          type="button"
          disabled={pending.resolving}
          onClick={() => {
            void onExecute(request).then((executed) => {
              if (executed && browserObserve) {
                onConsent();
              }
            });
          }}
        >
          {pending.resolving
            ? "Running…"
            : browserObserve && !browserConsent
              ? "Allow observation for this thread"
              : memorySearch
                ? "Run memory search"
                : "Approve and run"}
        </button>
      </div>
    </>
  );
}

function UserInputRequest({
  request,
  pending,
  onResolve,
  onDeny,
}: {
  request: Extract<ServerRequest, { method: "item/tool/requestUserInput" }>;
  pending: PendingServerRequest;
  onResolve: ServerRequestQueueProps["onResolve"];
  onDeny: ServerRequestQueueProps["onDeny"];
}) {
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [otherQuestions, setOtherQuestions] = useState<Set<string>>(
    () => new Set(),
  );
  const complete = request.params.questions.every(
    (question) => Boolean(answers[question.id]?.trim()),
  );
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        void onResolve(request.id, {
          answers: Object.fromEntries(
            request.params.questions.map((question) => [
              question.id,
              { answers: [answers[question.id] ?? ""] },
            ]),
          ),
        });
      }}
    >
      {request.params.questions.map((question) => (
        <fieldset key={question.id}>
          <legend>{question.header}</legend>
          <p>{question.question}</p>
          {question.options ? (
            <>
              {question.options.map((option) => (
                <label className="server-request-option" key={option.label}>
                  <input
                    type="radio"
                    name={`request-${String(request.id)}-${question.id}`}
                    value={option.label}
                    checked={
                      !otherQuestions.has(question.id) &&
                      answers[question.id] === option.label
                    }
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setOtherQuestions((current) => {
                        const next = new Set(current);
                        next.delete(question.id);
                        return next;
                      });
                      setAnswers((current) => ({
                        ...current,
                        [question.id]: value,
                      }));
                    }}
                  />
                  <span>
                    <strong>{option.label}</strong>
                    <small>{option.description}</small>
                  </span>
                </label>
              ))}
              {question.isOther && (
                <>
                  <label className="server-request-option">
                    <input
                      type="radio"
                      name={`request-${String(request.id)}-${question.id}`}
                      checked={otherQuestions.has(question.id)}
                      onChange={() => {
                        setOtherQuestions((current) =>
                          new Set(current).add(question.id),
                        );
                        setAnswers((current) => ({
                          ...current,
                          [question.id]: "",
                        }));
                      }}
                    />
                    <span>
                      <strong>Other</strong>
                      <small>Enter a different answer</small>
                    </span>
                  </label>
                  {otherQuestions.has(question.id) && (
                    <input
                      type={question.isSecret ? "password" : "text"}
                      aria-label={`Other answer: ${question.question}`}
                      value={answers[question.id] ?? ""}
                      autoFocus
                      onChange={(event) => {
                        const value = event.currentTarget.value;
                        setAnswers((current) => ({
                          ...current,
                          [question.id]: value,
                        }))
                      }}
                    />
                  )}
                </>
              )}
            </>
          ) : (
            <input
              type={question.isSecret ? "password" : "text"}
              aria-label={question.question}
              value={answers[question.id] ?? ""}
              onChange={(event) => {
                const value = event.currentTarget.value;
                setAnswers((current) => ({
                  ...current,
                  [question.id]: value,
                }))
              }}
            />
          )}
        </fieldset>
      ))}
      <div className="server-request-actions">
        <button
          type="button"
          disabled={pending.resolving}
          onClick={() => void onDeny(request.id)}
        >
          Deny
        </button>
        <button type="submit" disabled={pending.resolving || !complete}>
          Send answer
        </button>
      </div>
    </form>
  );
}

function McpElicitationRequest({
  request,
  pending,
  onResolve,
}: {
  request: Extract<ServerRequest, { method: "mcpServer/elicitation/request" }>;
  pending: PendingServerRequest;
  onResolve: ServerRequestQueueProps["onResolve"];
}) {
  const [content, setContent] = useState("{}");
  const [error, setError] = useState<string | null>(null);
  const params = request.params;
  function accept() {
    try {
      const parsed = params.mode === "url" ? null : JSON.parse(content);
      setError(null);
      void onResolve(request.id, {
        action: "accept",
        content: parsed,
        _meta: params._meta,
      });
    } catch {
      setError("Enter valid JSON before approving this MCP form.");
    }
  }
  return (
    <>
      <p>{params.message}</p>
      {params.mode === "url" ? (
        <p className="server-request-url">{params.url}</p>
      ) : (
        <label>
          Structured response (JSON)
          <textarea
            value={content}
            rows={4}
            onChange={(event) => setContent(event.currentTarget.value)}
          />
        </label>
      )}
      {error && <p role="alert">{error}</p>}
      <div className="server-request-actions">
        <button
          type="button"
          disabled={pending.resolving}
          onClick={() =>
            void onResolve(request.id, {
              action: "decline",
              content: null,
              _meta: params._meta,
            })
          }
        >
          Decline
        </button>
        <button type="button" disabled={pending.resolving} onClick={accept}>
          {params.mode === "url"
            ? "I completed this request"
            : "Approve MCP response"}
        </button>
      </div>
    </>
  );
}

function ServerRequestCard({
  pending,
  browserConsent,
  onConsent,
  onResolve,
  onDeny,
  onExecuteDynamicTool,
}: {
  pending: PendingServerRequest;
  browserConsent: boolean;
  onConsent: () => void;
  onResolve: ServerRequestQueueProps["onResolve"];
  onDeny: ServerRequestQueueProps["onDeny"];
  onExecuteDynamicTool: ServerRequestQueueProps["onExecuteDynamicTool"];
}) {
  const { request } = pending;
  return (
    <article data-method={request.method}>
      <header>
        <strong>{requestLabel(request)}</strong>
        <span>{pending.resolving ? "Resolving" : "Waiting"}</span>
      </header>
      {request.method === "item/commandExecution/requestApproval" && (
        <CommandApprovalRequestCard
          request={request}
          pending={pending}
          onResolve={onResolve}
          onDeny={onDeny}
        />
      )}
      {request.method === "item/fileChange/requestApproval" && (
        <>
          <p>{request.params.reason ?? "The agent wants to modify files."}</p>
          {request.params.grantRoot && <pre>{request.params.grantRoot}</pre>}
          {request.params.grantRoot && (
            <p role="status">
              This request includes a session-scoped write grant. Bridge does
              not offer persistent approval and can only deny it.
            </p>
          )}
          <div className="server-request-actions">
            <button
              type="button"
              disabled={pending.resolving}
              onClick={() =>
                void onResolve(request.id, { decision: "decline" })
              }
            >
              Deny
            </button>
            {!request.params.grantRoot && (
              <button
                type="button"
                disabled={pending.resolving}
                onClick={() =>
                  void onResolve(request.id, { decision: "accept" })
                }
              >
                Allow once
              </button>
            )}
          </div>
        </>
      )}
      {request.method === "item/permissions/requestApproval" && (
        <>
          <p>{request.params.reason ?? "The agent requested extra access."}</p>
          <pre>{pretty(request.params.permissions)}</pre>
          <div className="server-request-actions">
            <button
              type="button"
              disabled={pending.resolving}
              onClick={() => void onDeny(request.id)}
            >
              Deny
            </button>
            <button
              type="button"
              disabled={pending.resolving}
              onClick={() =>
                void onResolve(request.id, {
                  permissions: {
                    network: request.params.permissions.network ?? undefined,
                    fileSystem:
                      request.params.permissions.fileSystem ?? undefined,
                  },
                  scope: "turn",
                })
              }
            >
              Allow for turn
            </button>
          </div>
        </>
      )}
      {request.method === "item/tool/requestUserInput" && (
        <UserInputRequest
          request={request}
          pending={pending}
          onResolve={onResolve}
          onDeny={onDeny}
        />
      )}
      {request.method === "mcpServer/elicitation/request" && (
        <McpElicitationRequest
          request={request}
          pending={pending}
          onResolve={onResolve}
        />
      )}
      {request.method === "item/tool/call" && (
        <DynamicToolRequest
          request={request}
          pending={pending}
          browserConsent={browserConsent}
          onConsent={onConsent}
          onExecute={onExecuteDynamicTool}
          onDeny={onDeny}
        />
      )}
      {![
        "item/commandExecution/requestApproval",
        "item/fileChange/requestApproval",
        "item/permissions/requestApproval",
        "item/tool/requestUserInput",
        "mcpServer/elicitation/request",
        "item/tool/call",
      ].includes(request.method) && (
        <>
          <p>
            Bridge does not support this request type and will not approve it.
          </p>
          <button
            type="button"
            disabled={pending.resolving}
            onClick={() => void onDeny(request.id)}
          >
            Deny request
          </button>
        </>
      )}
      {pending.error && <p role="alert">{pending.error}</p>}
    </article>
  );
}

export function ServerRequestQueue({
  requests,
  onResolve,
  onDeny,
  onExecuteDynamicTool,
}: ServerRequestQueueProps) {
  const [browserConsentThreads, setBrowserConsentThreads] = useState<Set<string>>(
    () => new Set(),
  );
  if (requests.length === 0) {
    return null;
  }
  return (
    <section
      className="server-request-queue"
      aria-labelledby="server-request-heading"
      aria-live="polite"
    >
      <header>
        <div>
          <p className="eyebrow">Approval queue</p>
          <h2 id="server-request-heading">
            {requests.length} request{requests.length === 1 ? "" : "s"} waiting
          </h2>
        </div>
      </header>
      <div className="server-request-list">
        {requests.map((pending) => {
          const threadId = "threadId" in pending.request.params
            ? String(pending.request.params.threadId)
            : "unknown";
          return (
            <ServerRequestCard
              key={`${typeof pending.request.id}-${String(pending.request.id)}`}
              pending={pending}
              browserConsent={browserConsentThreads.has(threadId)}
              onConsent={() =>
                setBrowserConsentThreads((current) =>
                  new Set(current).add(threadId),
                )
              }
              onResolve={onResolve}
              onDeny={onDeny}
              onExecuteDynamicTool={onExecuteDynamicTool}
            />
          );
        })}
      </div>
    </section>
  );
}
