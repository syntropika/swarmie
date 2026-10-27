import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Check, ShieldCheck } from "lucide-react";

import { useSelection } from "../../client/use-selection";
import { Item, Menu } from "../../controls";
import { draftPermission, patchDraft } from "./state";

export function ComposerPermissions({ chat }: { chat: Conversation }) {
  const selected = useSelection(chat.draft, draftPermission);
  return (
    <Menu
      label="Permissions"
      icon={ShieldCheck}
      text={selected}
      size="composerCap"
      placement="top start"
    >
      {(close) => (
        <>
          {["Ask", "Full access"].map((permission) => (
            <Item
              key={permission}
              icon={permission === selected ? Check : ShieldCheck}
              onClick={() => {
                patchDraft(chat, { permission });
                close();
              }}
            >
              {permission}
            </Item>
          ))}
        </>
      )}
    </Menu>
  );
}
