import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { ShieldCheck } from "lucide-react";

import { useSelection } from "../../client/use-selection";
import { ChoiceList, Menu } from "../../controls";
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
        <ChoiceList
          label="Permissions"
          value={selected}
          items={["Ask", "Full access"].map((permission) => ({
            id: permission,
            label: permission,
            icon: ShieldCheck,
          }))}
          onAction={(permission) => {
            patchDraft(chat, { permission });
            close();
          }}
        />
      )}
    </Menu>
  );
}
