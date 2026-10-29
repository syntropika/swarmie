import { useSource } from "@swarmie/client-ui";
import { Code2, HelpCircle, LogIn, Settings } from "lucide-react";

import { useClient } from "../../client/context";
import { Item, Menu } from "../../controls";

export function AccountMenu() {
  const { navigate, openTab, shellState, plugins } = useClient();
  const installed = useSource(plugins);
  return (
    <Menu label="Account menu" icon={Settings} size="rail" placement="top start">
      {() => (
        <>
          <Item
            icon={Settings}
            onClick={() => {
              navigate({ section: "settings", settings: "general" });
              shellState.update((s) => ({ ...s, navigation: window.innerWidth >= 768 }));
            }}
          >
            Settings
          </Item>
          <Item
            icon={HelpCircle}
            onClick={() => {
              if (window.innerWidth < 768) {
                shellState.update((current) => ({ ...current, navigation: false }));
              }
              openTab("notes");
            }}
          >
            Help
          </Item>
          {installed.some((plugin) => plugin.id === "app-state" && plugin.enabled) ? (
            <Item
              icon={Code2}
              onClick={() => {
                if (window.innerWidth < 768) {
                  shellState.update((current) => ({ ...current, navigation: false }));
                }
                openTab("state");
              }}
            >
              App state
            </Item>
          ) : null}
          <div className="my-1 border-t border-border" />
          <Item
            icon={LogIn}
            onClick={() => {
              navigate({ section: "settings", settings: "general" });
              shellState.update((s) => ({ ...s, navigation: window.innerWidth >= 768 }));
            }}
          >
            Sign in to Swarmie
          </Item>
        </>
      )}
    </Menu>
  );
}
