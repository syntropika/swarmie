import { TextArea as HeroUITextArea } from "@heroui/react";
import type { TextAreaProps as HeroUITextAreaProps } from "@heroui/react";

import { cn } from "../classes";

export type TextAreaProps = HeroUITextAreaProps & {
  /** The enclosing composer provides its boundary; the editable area retains no focus ring. */
  appearance?: "default" | "composer";
};

export function TextArea({ appearance = "default", className, ...props }: TextAreaProps) {
  return (
    <HeroUITextArea
      {...props}
      className={cn(
        appearance === "composer" &&
          "block max-h-60 min-h-[52px] w-full resize-none rounded-none border-0 bg-transparent p-0 pb-2 text-caption leading-composer shadow-none ring-0 outline-none hover:bg-transparent focus:bg-transparent focus:shadow-none focus:ring-0 focus:outline-none focus-visible:ring-0 data-[focused=true]:bg-transparent data-[focused=true]:shadow-none data-[focused=true]:ring-0 data-[focused=true]:outline-none data-[hovered=true]:bg-transparent",
        className,
      )}
    />
  );
}
