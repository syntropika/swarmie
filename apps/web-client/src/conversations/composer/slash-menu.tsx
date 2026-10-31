import { useSource } from "@swarmie/client-ui";
import type { Conversation } from "@swarmie/plugin-conversations-browser";
import { Surface } from "@swarmie/ui-web";

import { useClient } from "../../client/context";
import { useSelection } from "../../client/use-selection";
import { Caption, Item } from "../../controls";
import { executeSlashCommand } from "./actions";
import type { ComposerActions } from "./actions";
import { draftText, isSlashCommand } from "./state";

export function ComposerSlashMenu({
  chat,
  executeConfirmed,
  actionContext,
}: { chat: Conversation } & Pick<ComposerActions, "executeConfirmed" | "actionContext">) {
  const { conversationsUi } = useClient();
  const text = useSelection(chat.draft, draftText);
  const commands = useSource(conversationsUi.composer.slashCommands);
  const slash = isSlashCommand(text);
  if (!slash) {
    return null;
  }
  const matches = commands.filter((command) => command.command.includes(text.slice(1)));
  return (
    <section aria-label="Slash commands" className="absolute inset-x-0 bottom-full z-30 mb-2">
      <Surface className="max-h-72 overflow-y-auto overscroll-contain">
        <Caption>Commands</Caption>
        {matches.map((command) => (
          <Item
            key={command.id}
            icon={command.icon}
            detail={`/${command.command}`}
            disabled={command.enabled?.(actionContext()) === false}
            onClick={() => {
              executeSlashCommand(chat, text, command, executeConfirmed).catch(() => undefined);
            }}
          >
            {command.label}
          </Item>
        ))}
        {matches.length === 0 ? (
          <p className="px-2 py-3 text-muted">No commands are available.</p>
        ) : null}
      </Surface>
    </section>
  );
}
