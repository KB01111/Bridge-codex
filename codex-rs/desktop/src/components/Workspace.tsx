import * as Tabs from "@radix-ui/react-tabs";

import type { BrowserFrame, BrowserStatus, DesktopStatus } from "../types";
import { BrowserWorkspace } from "./BrowserWorkspace";
import { DesktopWorkspace } from "./DesktopWorkspace";

export type WorkspaceProps = {
  browserStatus: BrowserStatus | null;
  browserFrame: BrowserFrame | null;
  desktopStatus: DesktopStatus | null;
  browserBusy: boolean;
  desktopBusy: boolean;
  browserError?: string | null;
  desktopError?: string | null;
  onStartBrowser: () => Promise<void>;
  onStopBrowser: () => Promise<void>;
  onNavigate: (url: string) => Promise<void>;
  onBrowserClickSelector: (selector: string) => Promise<void>;
  onBrowserClickAt: (x: number, y: number) => Promise<void>;
  onBrowserType: (selector: string, text: string) => Promise<void>;
  onRefreshDesktop: () => Promise<void>;
  onEnableDesktop: () => Promise<void>;
  onDisableDesktop: () => Promise<void>;
  onDesktopClickAt: (x: number, y: number) => Promise<void>;
  onDesktopType: (text: string) => Promise<void>;
};

export function Workspace(props: WorkspaceProps) {
  return (
    <main id="work-surfaces" className="workspace" aria-label="Work surfaces">
      <Tabs.Root
        className="workspace-tabs"
        defaultValue="browser"
        onValueChange={(value) => {
          if (value === "desktop") {
            void props.onRefreshDesktop();
          }
        }}
      >
        <Tabs.List aria-label="Work mode surfaces">
          <Tabs.Trigger value="browser">Agent browser</Tabs.Trigger>
          <Tabs.Trigger value="desktop">Desktop control</Tabs.Trigger>
        </Tabs.List>
        <Tabs.Content value="browser">
          <BrowserWorkspace
            status={props.browserStatus}
            frame={props.browserFrame}
            busy={props.browserBusy}
            error={props.browserError}
            onStart={props.onStartBrowser}
            onStop={props.onStopBrowser}
            onNavigate={props.onNavigate}
            onClickSelector={props.onBrowserClickSelector}
            onClickAt={props.onBrowserClickAt}
            onType={props.onBrowserType}
          />
        </Tabs.Content>
        <Tabs.Content value="desktop">
          <DesktopWorkspace
            status={props.desktopStatus}
            busy={props.desktopBusy}
            error={props.desktopError}
            onRefresh={props.onRefreshDesktop}
            onEnable={props.onEnableDesktop}
            onDisable={props.onDisableDesktop}
            onClickAt={props.onDesktopClickAt}
            onType={props.onDesktopType}
          />
        </Tabs.Content>
      </Tabs.Root>
    </main>
  );
}
