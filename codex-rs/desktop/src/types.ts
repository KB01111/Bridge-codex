export type ServiceStatus = {
  running: boolean;
  error?: string | null;
};

export type ProxyStatus = ServiceStatus & {
  baseUrl: string;
  authenticated: boolean;
  responsesApi: boolean;
  compatibility: "unavailable" | "basic" | "conformant";
  probedModel?: string | null;
  experimentalModelCount: number;
};

export type A2aStatus = ServiceStatus & {
  enabled: boolean;
  address: string;
  tokenConfigured: boolean;
};

export type A2aServerSettings = {
  enabled: boolean;
  port: number;
};

export type A2aTokenProvisioning = {
  token: string;
  status: A2aStatus;
};

export type ProxyModel = {
  id: string;
  object?: string | null;
  ownedBy?: string | null;
  classification: "known" | "experimental";
};

export type BrowserHealth =
  | "stopped"
  | "starting"
  | "running"
  | "degraded"
  | "failed";

export type BrowserStatus = {
  running: boolean;
  url?: string | null;
  viewportWidth: number;
  viewportHeight: number;
  health: BrowserHealth;
  error?: string | null;
};

export type BrowserFrame = {
  jpegBase64: string;
  url: string;
  width: number;
  height: number;
  sequence: number;
  capturedAt: string;
};

export type DesktopStatus = {
  available: boolean;
  enabled: boolean;
  platform: string;
  displaySize?: [number, number] | null;
  error?: string | null;
};

export type A2aTaskState =
  | "TASK_STATE_WORKING"
  | "TASK_STATE_COMPLETED"
  | "TASK_STATE_FAILED"
  | "TASK_STATE_CANCELED";

export type A2aTextPart = {
  text: string;
};

export type A2aMessage = {
  messageId: string;
  role: "ROLE_USER" | "ROLE_AGENT";
  parts: A2aTextPart[];
};

export type A2aTask = {
  id: string;
  contextId: string;
  status: {
    state: A2aTaskState;
    timestamp: string;
    message?: A2aMessage | null;
  };
  artifacts: Array<{
    artifactId: string;
    name: string;
    parts: A2aTextPart[];
  }>;
};

export type DelegateA2aTaskRequest = {
  prompt: string;
  model?: string | null;
  contextId?: string | null;
};

export type ValidationIssue = {
  severity: "error";
  code: string;
  message: string;
  line?: number | null;
};

export type ValidatedCodeBlock = {
  language: string;
  target?: string | null;
  rootKind: string;
  sourceBytes: number;
  stdoutDetected: boolean;
};

export type CodeValidation = {
  valid: boolean;
  containsCode: boolean;
  expectedStdoutDescription: boolean;
  blocks: ValidatedCodeBlock[];
  issues: ValidationIssue[];
};

export type SandboxValidationEvent = {
  requestId: string;
  validation: CodeValidation;
};

export type CodePolicyStatus = {
  parser: string;
  languageAbiVersion: number;
  supportedLanguages: string[];
  compiledTarget: string;
  networkAccess: string;
  sandboxRoot: string;
  responseContract: string;
};

export type CodeMemoryLanguage =
  | "c"
  | "cpp"
  | "go"
  | "javaScript"
  | "typeScript"
  | "tsx"
  | "python"
  | "rust"
  | "swift";

export type CodeMemoryStatistics = {
  discoveredEntries: number;
  discoveredFiles: number;
  indexedFiles: number;
  skippedFiles: number;
  sourceBytes: number;
  chunks: number;
  syntaxIssues: number;
  symbols: number;
  graphEdges: number;
};

export type CodeMemoryStatus = {
  ready: boolean;
  indexing: boolean;
  root?: string | null;
  indexedAt?: string | null;
  schemaVersion: number;
  parser: string;
  retrieval: string;
  storage: string;
  networkAccess: string;
  statistics: CodeMemoryStatistics;
  error?: string | null;
};

export type CodeMemoryIndexResult = {
  status: CodeMemoryStatus;
  warnings: string[];
};

export type CodeMemorySearchRequest = {
  query: string;
  maxResults: number;
  graphWeight: number;
};

export type CodeMemoryGraphRelation = "import" | "reference" | "sameFile";

export type CodeMemoryGraphExplanation = {
  fromChunkId: string;
  relation: CodeMemoryGraphRelation;
  contribution: number;
};

export type CodeMemoryChunk = {
  id: string;
  path: string;
  language: CodeMemoryLanguage;
  symbol?: string | null;
  kind: string;
  startByte: number;
  endByte: number;
  startLine: number;
  endLine: number;
  source: string;
};

export type CodeMemorySearchResult = {
  chunk: CodeMemoryChunk;
  score: number;
  lexicalScore: number;
  graphScore: number;
  explanations: CodeMemoryGraphExplanation[];
};
