import {
  Bot,
  Database,
  Globe,
  ListTodo,
  LoaderCircle,
  MessageSquareText,
  Monitor,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Router,
  Sun,
  type LucideIcon,
} from "lucide-react";

import type { A2aStatus, A2aTask, BrowserStatus, ProxyStatus } from "../types";
import type { ConversationSummary } from "../useBridgeState";

export type ToolView = "browser" | "desktop" | "routing" | "tasks" | "memory";

export type AppNavigationProps = {
  activeTool: ToolView | null;
  collapsed: boolean;
  theme: "light" | "dark";
  proxyStatus: ProxyStatus | null;
  browserStatus: BrowserStatus | null;
  a2aStatus: A2aStatus | null;
  a2aTasks: A2aTask[];
  conversations: ConversationSummary[];
  activeConversationId: string;
  selectedModel: string;
  sending: boolean;
  onToggleCollapsed: () => void;
  onToggleTheme: () => void;
  onSelectTool: (tool: ToolView) => void;
  onNewTask: () => void;
  onSelectConversation: (id: string) => void;
};

type ToolNavigationItem = {
  id: ToolView;
  label: string;
  description: string;
  icon: LucideIcon;
};

type ServiceState = "checking" | "online" | "offline" | "degraded";

const primaryTools: ToolNavigationItem[] = [
  {
    id: "browser",
    label: "Agent browser",
    description: "Open the isolated browser workspace",
    icon: Globe,
  },
  {
    id: "desktop",
    label: "Desktop control",
    description: "Control the host desktop when Work Mode is enabled",
    icon: Monitor,
  },
];

const utilityTools: ToolNavigationItem[] = [
  {
    id: "tasks",
    label: "Delegated tasks",
    description: "Review work delegated through the local A2A endpoint",
    icon: ListTodo,
  },
  {
    id: "memory",
    label: "Code memory",
    description: "Index and search local source code",
    icon: Database,
  },
  {
    id: "routing",
    label: "Models & routing",
    description: "Manage the model router, account, and execution policy",
    icon: Router,
  },
];

function conversationTimestamp(timestamp: string): string | null {
  const value = new Date(timestamp);
  if (Number.isNaN(value.getTime())) {
    return null;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);
}

function basicServiceState(running: boolean | null): ServiceState {
  if (running === null) {
    return "checking";
  }
  return running ? "online" : "offline";
}

function browserServiceState(status: BrowserStatus | null): ServiceState {
  if (!status) {
    return "checking";
  }
  if (status.health === "degraded" || status.health === "starting") {
    return "degraded";
  }
  return status.running ? "online" : "offline";
}

function serviceStateLabel(state: ServiceState): string {
  switch (state) {
    case "checking":
      return "Checking";
    case "online":
      return "Online";
    case "offline":
      return "Offline";
    case "degraded":
      return "Degraded";
  }
}

function ToolButton({
  item,
  active,
  collapsed,
  count,
  onSelect,
}: {
  item: ToolNavigationItem;
  active: boolean;
  collapsed: boolean;
  count?: number;
  onSelect: (tool: ToolView) => void;
}) {
  const Icon = item.icon;
  const countLabel = count === undefined ? null : count > 99 ? "99+" : count;
  const accessibleLabel =
    count === undefined ? item.label : `${item.label}, ${count} total`;

  return (
    <button
      className="app-navigation__item"
      type="button"
      data-active={active || undefined}
      aria-current={active ? "page" : undefined}
      aria-label={accessibleLabel}
      title={collapsed ? item.description : undefined}
      onClick={() => onSelect(item.id)}
    >
      <Icon aria-hidden="true" size={18} strokeWidth={1.8} />
      {!collapsed && (
        <>
          <span className="app-navigation__item-label">{item.label}</span>
          {countLabel !== null && (
            <span className="app-navigation__count" aria-hidden="true">
              {countLabel}
            </span>
          )}
        </>
      )}
    </button>
  );
}

export function AppNavigation({
  activeTool,
  collapsed,
  theme,
  proxyStatus,
  browserStatus,
  a2aStatus,
  a2aTasks,
  conversations,
  activeConversationId,
  selectedModel,
  sending,
  onToggleCollapsed,
  onToggleTheme,
  onSelectTool,
  onNewTask,
  onSelectConversation,
}: AppNavigationProps) {
  const conversationList = conversations;
  const proxyState = basicServiceState(proxyStatus?.running ?? null);
  const browserState = browserServiceState(browserStatus);
  const a2aState = basicServiceState(a2aStatus?.running ?? null);
  const modelName = selectedModel || "No model selected";
  const modelState = sending
    ? "Responding"
    : !selectedModel
      ? "Choose a model in Routing"
      : proxyState === "online"
        ? "Ready"
        : proxyState === "checking"
          ? "Checking router"
          : "Router offline";
  const nextTheme = theme === "dark" ? "light" : "dark";
  const ThemeIcon = theme === "dark" ? Sun : Moon;

  const services = [
    { label: "Router", state: proxyState },
    { label: "Browser", state: browserState },
    { label: "A2A", state: a2aState },
  ] as const;

  return (
    <aside
      className="app-navigation"
      data-collapsed={collapsed || undefined}
      aria-label="Bridge Codex navigation"
    >
      <header className="app-navigation__header">
        <div className="app-navigation__brand" aria-label="Bridge Codex">
          <span className="app-navigation__brand-mark" aria-hidden="true">
            <Bot size={19} strokeWidth={1.8} />
          </span>
          {!collapsed && <strong>Bridge Codex</strong>}
        </div>
        <button
          className="app-navigation__icon-button"
          type="button"
          aria-label={collapsed ? "Expand navigation" : "Collapse navigation"}
          aria-expanded={!collapsed}
          title={collapsed ? "Expand navigation" : "Collapse navigation"}
          onClick={onToggleCollapsed}
        >
          {collapsed ? (
            <PanelLeftOpen aria-hidden="true" size={18} />
          ) : (
            <PanelLeftClose aria-hidden="true" size={18} />
          )}
        </button>
      </header>

      <div className="app-navigation__body">
        <button
          className="app-navigation__new-task"
          type="button"
          aria-label="Start a new conversation"
          title={collapsed ? "New conversation" : undefined}
          disabled={sending}
          onClick={onNewTask}
        >
          <Plus aria-hidden="true" size={18} strokeWidth={2} />
          {!collapsed && <span>New conversation</span>}
        </button>

        <nav className="app-navigation__tools" aria-label="Workspaces">
          {!collapsed && (
            <p className="app-navigation__section-label">Workspaces</p>
          )}
          {primaryTools.map((item) => (
            <ToolButton
              key={item.id}
              item={item}
              active={activeTool === item.id}
              collapsed={collapsed}
              onSelect={onSelectTool}
            />
          ))}

          {!collapsed && (
            <p className="app-navigation__section-label">Local tools</p>
          )}
          {utilityTools.map((item) => (
            <ToolButton
              key={item.id}
              item={item}
              active={activeTool === item.id}
              collapsed={collapsed}
              count={item.id === "tasks" ? a2aTasks.length : undefined}
              onSelect={onSelectTool}
            />
          ))}
        </nav>

        {!collapsed && (
          <section
            className="app-navigation__recent"
            aria-labelledby="conversations-heading"
          >
            <div className="app-navigation__section-heading">
              <h2 id="conversations-heading">Conversations</h2>
            </div>
            {conversationList.length === 0 ? (
              <p className="app-navigation__empty">
                Your local conversations will appear here.
              </p>
            ) : (
              <ol className="app-navigation__task-list">
                {conversationList.map((conversation) => {
                  const updatedAt = conversationTimestamp(
                    conversation.updatedAt,
                  );
                  const active = conversation.id === activeConversationId;
                  return (
                    <li key={conversation.id}>
                      <button
                        type="button"
                        data-active={active || undefined}
                        aria-current={active ? "page" : undefined}
                        title={conversation.title}
                        disabled={sending && !active}
                        onClick={() => onSelectConversation(conversation.id)}
                      >
                        <MessageSquareText
                          className="app-navigation__task-state"
                          aria-hidden="true"
                          size={15}
                        />
                        <span className="app-navigation__task-copy">
                          <strong>{conversation.title}</strong>
                          <span>
                            {active ? "Current" : "Local"}
                            {updatedAt ? ` · ${updatedAt}` : ""}
                          </span>
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ol>
            )}
          </section>
        )}
      </div>

      <footer className="app-navigation__footer">
        <div
          className="app-navigation__model"
          aria-label={`${modelName}. ${modelState}.`}
          title={collapsed ? `${modelName} · ${modelState}` : undefined}
        >
          {sending ? (
            <LoaderCircle
              className="app-navigation__model-working"
              aria-hidden="true"
              size={18}
            />
          ) : (
            <Bot aria-hidden="true" size={18} strokeWidth={1.8} />
          )}
          {!collapsed && (
            <span>
              <strong title={selectedModel || undefined}>{modelName}</strong>
              <small>{modelState}</small>
            </span>
          )}
        </div>

        <ul className="app-navigation__services" aria-label="Local services">
          {services.map((service) => (
            <li
              key={service.label}
              data-state={service.state}
              aria-label={`${service.label}: ${serviceStateLabel(service.state)}`}
              title={`${service.label}: ${serviceStateLabel(service.state)}`}
            >
              <span aria-hidden="true" />
              {!collapsed && <small>{service.label}</small>}
            </li>
          ))}
        </ul>

        <button
          className="app-navigation__theme-toggle"
          type="button"
          aria-label={`Use ${nextTheme} theme`}
          title={`Use ${nextTheme} theme`}
          onClick={onToggleTheme}
        >
          <ThemeIcon aria-hidden="true" size={17} strokeWidth={1.8} />
          {!collapsed && (
            <span>{theme === "dark" ? "Light" : "Dark"} theme</span>
          )}
        </button>
      </footer>
    </aside>
  );
}
