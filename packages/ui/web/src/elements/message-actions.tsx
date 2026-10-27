import type { ComponentProps } from "react";

import { ChromeButton } from "../atoms/chrome-button";
import { Icon } from "../atoms/icon";
import { cn } from "../classes";

export type Reaction = "up" | "down" | null;
type Presentation = {
  /** Hover mode expects a group/message ancestor and remains available to keyboard and touch. */
  readonly visibility?: "always" | "hover";
  readonly align?: "start" | "end";
};
type ContainerProps = Omit<ComponentProps<"div">, "children" | "onCopy"> & Presentation;
type ControlledActions = ContainerProps & {
  copied: boolean;
  reaction: Reaction;
  regenerating: boolean;
  onCopy: () => void;
  onReactionChange: (reaction: Reaction) => void;
  onRegenerate: () => void;
  onMore: () => void;
};

/** The presentation port also accepts composed controls owned by its consumer or plugins. */
export type MessageActionsProps = (ComponentProps<"div"> & Presentation) | ControlledActions;

/** Compact message actions retain the controlled Elements API and allow custom action content. */
export function MessageActions(props: MessageActionsProps) {
  if ("copied" in props) {
    const {
      copied,
      reaction,
      regenerating,
      onCopy,
      onReactionChange,
      onRegenerate,
      onMore,
      ...container
    } = props;
    return (
      <MessageActions {...container}>
        <ChromeButton
          size="composer"
          aria-label={copied ? "Copied response" : "Copy response"}
          onPress={onCopy}
        >
          <Icon name={copied ? "check" : "copy"} />
        </ChromeButton>
        <ChromeButton
          size="composer"
          aria-label="Mark response helpful"
          aria-pressed={reaction === "up"}
          onPress={() => {
            onReactionChange(reaction === "up" ? null : "up");
          }}
        >
          <Icon name="thumbs-up" />
        </ChromeButton>
        <ChromeButton
          size="composer"
          aria-label="Mark response unhelpful"
          aria-pressed={reaction === "down"}
          onPress={() => {
            onReactionChange(reaction === "down" ? null : "down");
          }}
        >
          <Icon name="thumbs-down" />
        </ChromeButton>
        <ChromeButton size="composer" aria-label="Regenerate response" onPress={onRegenerate}>
          <Icon
            name="refresh"
            className={regenerating ? "animate-spin motion-reduce:animate-none" : undefined}
          />
        </ChromeButton>
        <ChromeButton size="composer" aria-label="More response actions" onPress={onMore}>
          <Icon name="dots" />
        </ChromeButton>
      </MessageActions>
    );
  }
  const { children, className, visibility = "always", align = "start", ...container } = props;
  return (
    <div
      {...container}
      data-slot="message-actions"
      className={cn(
        "flex flex-wrap items-center gap-1.5",
        align === "end" && "justify-end",
        visibility === "hover" &&
          "pointer-events-none opacity-0 group-focus-within/message:pointer-events-auto group-focus-within/message:opacity-100 group-hover/message:pointer-events-auto group-hover/message:opacity-100 has-[[aria-expanded=true]]:pointer-events-auto has-[[aria-expanded=true]]:opacity-100 [@media(hover:none)]:pointer-events-auto [@media(hover:none)]:opacity-100",
        className,
      )}
    >
      {children}
    </div>
  );
}
