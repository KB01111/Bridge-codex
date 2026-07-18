import * as Tabs from "@radix-ui/react-tabs";
import type { Dispatch } from "react";

import type { BridgeAgentRuntimeStatus } from "../agentRuntime";
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
import type {
  LocalDataAction,
  LocalDataState,
} from "../localDataReducer";
import { MemoryPanel } from "./MemoryPanel";
import { RoutingPanel } from "./RoutingPanel";
import { TasksPanel } from "./TasksPanel";

export type ControlPanelProps = {
  activeTab: "routing" | "tasks" | "memory";
  compact?: boolean;
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
  runtimeStatus: BridgeAgentRuntimeStatus | null;
  routingBusy: boolean;
  a2aBusy: boolean;
  codeMemoryBusy: boolean;
  routingError?: string | null;
  a2aError?: string | null;
  codeMemoryError?: string | null;
  onRefreshAll: () => Promise<void>;
  onSelectModel: (model: string) => void;
  onRefreshModels: () => Promise<void>;
  onConfigureProxy: (baseUrl: string, apiKey: string) => Promise<void>;
  onRefreshA2aTasks: () => Promise<void>;
  onSelectA2aTask: (id: string | null) => void;
  onDelegateA2aTask: (request: DelegateA2aTaskRequest) => Promise<void>;
  onCancelA2aTask: (id: string) => Promise<void>;
  onConfigureA2aServer: (enabled: boolean, port: number) => Promise<unknown>;
  onProvisionA2aToken: (regenerate: boolean) => Promise<string | null>;
  onDeleteA2aToken: () => Promise<unknown>;
  onRefreshCodeMemory: () => Promise<void>;
  onIndexCodeMemory: (root: string) => Promise<void>;
  onSearchCodeMemory: (request: CodeMemorySearchRequest) => Promise<void>;
  onClearCodeMemory: () => Promise<void>;
  onClearCodeMemoryResults: () => void;
  localDataState: LocalDataState;
  dispatchLocalData: Dispatch<LocalDataAction>;
  conversationCount: number;
  currentMessageCount: number;
  localDataBusy: boolean;
  onExportSession: () => void;
  onArchiveCurrent: () => void;
  onDeleteAllLocalData: () => Promise<boolean>;
  onExportSupportBundle: () => Promise<string>;
  onActiveTabChange: (value: "routing" | "tasks" | "memory") => void;
};

export function ControlPanel(props: ControlPanelProps) {
  const busy = props.routingBusy || props.a2aBusy || props.codeMemoryBusy;

  return (
    <div id="control-panel" className="control-panel">
      {!props.compact && (
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
      )}

      <Tabs.Root
        className="control-panel-tabs"
        value={props.activeTab}
        onValueChange={(value) => {
          if (value === "tasks") {
            void props.onRefreshA2aTasks();
          } else if (value === "memory") {
            void props.onRefreshCodeMemory();
          }
          if (value === "routing" || value === "tasks" || value === "memory") {
            props.onActiveTabChange(value);
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
            runtimeStatus={props.runtimeStatus}
            busy={props.routingBusy}
            error={props.routingError}
            onSelectModel={props.onSelectModel}
            onRefreshModels={props.onRefreshModels}
            onConfigureProxy={props.onConfigureProxy}
            localDataState={props.localDataState}
            dispatchLocalData={props.dispatchLocalData}
            conversationCount={props.conversationCount}
            currentMessageCount={props.currentMessageCount}
            localDataBusy={props.localDataBusy}
            onExportSession={props.onExportSession}
            onArchiveCurrent={props.onArchiveCurrent}
            onDeleteAllLocalData={props.onDeleteAllLocalData}
            onExportSupportBundle={props.onExportSupportBundle}
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
            onConfigureServer={props.onConfigureA2aServer}
            onProvisionToken={props.onProvisionA2aToken}
            onDeleteToken={props.onDeleteA2aToken}
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
    </div>
  );
}
