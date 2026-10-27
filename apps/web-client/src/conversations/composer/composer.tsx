import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { useState } from "react";

import { useComposerActions } from "./actions";
import { ComposerAddMenu } from "./add-menu";
import { ComposerAttachments } from "./attachments";
import { ComposerDelivery } from "./delivery";
import { ComposerInput } from "./input";
import { ComposerModel } from "./model";
import { ComposerPermissions } from "./permissions";
import { ComposerPluginActions } from "./plugin-actions";
import { ComposerSlashMenu } from "./slash-menu";
import type { DeliveryMode } from "./state";
import { ComposerStatus } from "./status";
import { ComposerSubmit } from "./submit";
import { ComposerTopBar } from "./top-bar";

export function Composer({ chat }: { chat: Conversation }) {
  const actions = useComposerActions(chat);
  const [mode, setMode] = useState<DeliveryMode>("queue");
  return (
    <div className="@container/composer shrink-0 px-4 pt-2 pb-4 md:px-6 md:pb-5">
      <div className="relative mx-auto w-full max-w-[720px]">
        <ComposerSlashMenu chat={chat} executeAction={actions.executeAction} />
        <div className="overflow-hidden rounded-xl border border-composer-border bg-composer">
          <ComposerTopBar chat={chat} />
          <ComposerAttachments chat={chat} />
          <ComposerInput chat={chat} mode={mode} />
          <div className="flex items-center gap-1 px-2 pb-2">
            <ComposerAddMenu chat={chat} executeAction={actions.executeAction} />
            <ComposerPermissions chat={chat} />
            <ComposerPluginActions chat={chat} slot="leading" actions={actions} />
            <div className="flex-1" />
            <ComposerDelivery chat={chat} mode={mode} setMode={setMode} />
            <ComposerModel chat={chat} />
            <ComposerPluginActions chat={chat} slot="before-send" actions={actions} />
            <ComposerSubmit chat={chat} mode={mode} />
          </div>
        </div>
        <ComposerStatus chat={chat} />
      </div>
    </div>
  );
}
