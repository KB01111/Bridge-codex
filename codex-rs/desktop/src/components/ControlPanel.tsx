import * as Tabs from "@radix-ui/react-tabs";

import type {
  A2aStatus,
  A2aTask,
  CodeMemorySearchRequest,
  CodeMemorySearchResult,
  CodeMemoryStatus,
  CodePolicyStatus,
  DelegateA2aTaskRequest,
  ProxyModel,
  ProxyStatus,
} from "../types";
import type { LoginState, LoginVerificationState } from "../useBridgeState";
import { MemoryPanel } from "./MemoryPanel";
import { RoutingPanel } from "./RoutingPanel";
import { TasksPanel } from "./TasksPanel";

export type ControlPanelProps = {
  proxyStatus: ProxyStatus | null;
  a2aStatus: A2aStatus | null;
  a2aTasks: A2aTask[];
  selectedA2aTask: A2aTask | null;
  codePolicyStatus: CodePolicyStatus | null;
  codeMemoryStatus: CodeMemoryStatus | null;
  codeMemoryResults: CodeMemorySearchResult[];
  codeMemoryWarnings: string[];
  models: ProxyModel[];
  selectedModel: string;
  loginState: LoginState;
  loginVerification: LoginVerificationState;
  routingBusy: boolean;
  a2aBusy: boolean;
  codeMemoryBusy: boolean;
  routingError?: string | null;
  a2aError?: string | null;
  codeMemoryError?: string | null;
  onRefreshAll: () => Promise<void>;
  onSelectModel: (model: string) => void;
  onRefreshModels: () => Promise<void>;
  onEnsureProxy: () => Promise<void>;
  onLaunchLogin: () => Promise<void>;
  onVerifyLogin: () => Promise<void>;
  onResetLogin: () => void;
  onRefreshA2aTasks: () => Promise<void>;
  onSelectA2aTask: (id: string | null) => void;
  onDelegateA2aTask: (request: DelegateA2aTaskRequest) => Promise<void>;
  onCancelA2aTask: (id: string) => Promise<void>;
  onRefreshCodeMemory: () => Promise<void>;
  onIndexCodeMemory: (root: string) => Promise<void>;
  onSearchCodeMemory: (request: CodeMemorySearchRequest) => Promise<void>;
  onClearCodeMemory: () => Promise<void>;
  onClearCodeMemoryResults: () => void;
};

export function ControlPanel(props: ControlPanelProps) {
  const busy = props.routingBusy || props.a2aBusy || props.codeMemoryBusy;

  return (
    <aside
      id="control-panel"
      className="control-panel"
      aria-label="Work mode controls"
    >
      <header>
        <div>
          <p className="eyebrow">Local orchestration</p>
          <h2>Control panel</h2>
        </div>
        <button
          type="button"
          disabled={busy}
          onClick={() => void props.onRefreshAll()}
        >
          {busy ? "Working…" : "Refresh all"}
        </button>
      </header>

      <Tabs.Root
        className="control-panel-tabs"
        defaultValue="routing"
        onValueChange={(value) => {
          if (value === "tasks") {
            void props.onRefreshA2aTasks();
          } else if (value === "memory") {
            void props.onRefreshCodeMemory();
          }
        }}
      >
        <Tabs.List aria-label="Control panel sections">
          <Tabs.Trigger value="routing">Routing</Tabs.Trigger>
          <Tabs.Trigger value="tasks">
            Tasks{" "}
            <span aria-label={`${props.a2aTasks.length} tasks`}>
              {props.a2aTasks.length}
            </span>
          </Tabs.Trigger>
          <Tabs.Trigger value="memory">
            Memory
            {props.codeMemoryStatus?.ready && (
              <span className="ready-indicator">Ready</span>
            )}
          </Tabs.Trigger>
        </Tabs.List>

        <Tabs.Content value="routing">
          <RoutingPanel
            proxyStatus={props.proxyStatus}
            codePolicyStatus={props.codePolicyStatus}
            models={props.models}
            selectedModel={props.selectedModel}
            loginState={props.loginState}
            loginVerification={props.loginVerification}
            busy={props.routingBusy}
            error={props.routingError}
            onSelectModel={props.onSelectModel}
            onRefreshModels={props.onRefreshModels}
            onEnsureProxy={props.onEnsureProxy}
            onLaunchLogin={props.onLaunchLogin}
            onVerifyLogin={props.onVerifyLogin}
            onResetLogin={props.onResetLogin}
          />
        </Tabs.Content>

        <Tabs.Content value="tasks">
          <TasksPanel
            status={props.a2aStatus}
            tasks={props.a2aTasks}
            selectedTask={props.selectedA2aTask}
            models={props.models}
            defaultModel={props.selectedModel}
            busy={props.a2aBusy}
            error={props.a2aError}
            onRefresh={props.onRefreshA2aTasks}
            onSelectTask={props.onSelectA2aTask}
            onDelegate={props.onDelegateA2aTask}
            onCancel={props.onCancelA2aTask}
          />
        </Tabs.Content>

        <Tabs.Content value="memory">
          <MemoryPanel
            status={props.codeMemoryStatus}
            results={props.codeMemoryResults}
            warnings={props.codeMemoryWarnings}
            busy={props.codeMemoryBusy}
            error={props.codeMemoryError}
            onRefresh={props.onRefreshCodeMemory}
            onIndex={props.onIndexCodeMemory}
            onSearch={props.onSearchCodeMemory}
            onClear={props.onClearCodeMemory}
            onClearResults={props.onClearCodeMemoryResults}
          />
        </Tabs.Content>
      </Tabs.Root>
    </aside>
  );
}
