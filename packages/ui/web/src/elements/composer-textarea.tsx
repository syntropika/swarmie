import type { ComponentProps } from "react";

import { TextArea } from "../atoms/textarea";
import { cn } from "../classes";

export type ComposerTextAreaProps = ComponentProps<typeof TextArea>;

/** The Composer owns its boundary; the host owns text-driven height changes. */
export function ComposerTextArea({ className, ...props }: ComposerTextAreaProps) {
  return (
    <TextArea
      {...props}
      data-appearance="composer"
      className={cn(
        "block max-h-(--spacing-composer-input-max) min-h-(--spacing-composer-input) w-full resize-none overflow-y-auto rounded-none border-0 bg-transparent px-(--spacing-composer-x) pt-(--spacing-composer-top) pb-(--spacing-composer-bottom) text-conversation leading-(--leading-composer-input) shadow-none ring-0 outline-none hover:bg-transparent focus:bg-transparent focus:shadow-none focus:ring-0 focus:outline-none focus-visible:ring-0 data-[focused=true]:bg-transparent data-[focused=true]:shadow-none data-[focused=true]:ring-0 data-[focused=true]:outline-none data-[hovered=true]:bg-transparent",
        className,
      )}
    />
  );
}
