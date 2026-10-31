import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { ComposerPanel } from "@swarmie/ui-web/elements/conversation";
import { useState } from "react";

import { useComposerActions } from "./actions";
import { ComposerAddMenu } from "./add-menu";
import { ComposerAttachments } from "./attachments";
import { ComposerDelivery } from "./delivery";
import { ComposerFeedback } from "./feedback";
import { ComposerInput } from "./input";
import { ComposerModel } from "./model";
import { ComposerPluginActions } from "./plugin-actions";
import { ComposerSlashMenu } from "./slash-menu";
import type { DeliveryMode } from "./state";
import { ComposerSubmit } from "./submit";
import { ComposerTopBar } from "./top-bar";

export function Composer({ chat, showStatus }: { chat: Conversation; showStatus: () => void }) {
  const actions = useComposerActions(chat);
  const [mode, setMode] = useState<DeliveryMode>("queue");
  return (
    <div className="@container/composer shrink-0 px-4 pt-2 pb-2 md:px-6 md:pb-5">
      <div className="relative mx-auto w-full max-w-[720px]">
        <ComposerSlashMenu
          chat={chat}
          executeConfirmed={actions.executeConfirmed}
          actionContext={actions.actionContext}
        />
        <ComposerFeedback chat={chat} showStatus={showStatus} />
        <ComposerPanel>
          <ComposerTopBar chat={chat} />
          <ComposerPanel.Body>
            <ComposerAttachments chat={chat} />
            <ComposerInput chat={chat} mode={mode} actions={actions} />
            <div className="flex flex-wrap items-center gap-1 px-1 pb-1">
              <ComposerAddMenu chat={chat} actions={actions} />
              <div className="hidden @min-[400px]/composer:contents">
                <ComposerPluginActions chat={chat} slot="leading" actions={actions} />
              </div>
              <ComposerDelivery chat={chat} mode={mode} setMode={setMode} />
              <div className="flex min-w-0 flex-1 justify-end">
                <ComposerModel chat={chat} />
              </div>
              <div className="hidden @min-[400px]/composer:contents">
                <ComposerPluginActions chat={chat} slot="before-send" actions={actions} />
              </div>
              <ComposerSubmit chat={chat} mode={mode} />
            </div>
          </ComposerPanel.Body>
        </ComposerPanel>
      </div>
    </div>
  );
}
