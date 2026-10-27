import { HelpCircle, LogIn, Settings } from "lucide-react";

import { useClient } from "../../client/context";
import { Item, Menu } from "../../controls";

export function AccountMenu() {
  const { navigate, openTab, shellState } = useClient();
  return (
    <Menu label="Account menu" icon={Settings} size="rail" placement="top start">
      {(close) => (
        <>
          <Item
            icon={Settings}
            onClick={() => {
              navigate({ section: "settings" });
              shellState.update((s) => ({ ...s, navigation: window.innerWidth >= 768 }));
              close();
            }}
          >
            Settings
          </Item>
          <Item
            icon={HelpCircle}
            onClick={() => {
              openTab("notes");
              close();
            }}
          >
            Help
          </Item>
          <div className="my-1 border-t border-border" />
          <Item
            icon={LogIn}
            onClick={() => {
              navigate({ section: "settings", settings: "general" });
              close();
            }}
          >
            Sign in to Swarmie
          </Item>
        </>
      )}
    </Menu>
  );
}
