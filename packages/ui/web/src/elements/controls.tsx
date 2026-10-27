import { mergeProps, mergeRefs } from "@react-aria/utils";
import type { ComponentPropsWithRef } from "react";

import { Button } from "../atoms/button";
import { Input } from "../atoms/input";
import { TextArea } from "../atoms/textarea";
import { definedProps } from "./defined-props";
import { cn } from "./utilities";

function layoutClasses(className?: string) {
  return className
    ?.split(/\s+/u)
    .filter((token) =>
      /^(?:flex(?:-1|-col|-wrap)?|grid|grid-cols-.+|items-.+|justify-.+|w-full|min-w-0|max-w-.+|text-start|relative|aspect-.+|overflow-hidden)$/u.test(
        token,
      ),
    )
    .join(" ");
}

/** Preserve native action props and content layout on shared HeroUI controls. Rich labels may wrap. */
export function ElementButton({
  className,
  children,
  disabled,
  type,
  ref,
  ...props
}: ComponentPropsWithRef<"button">) {
  const classes = new Set(className?.split(/\s+/u));
  const primary = classes.has("bg-foreground") || classes.has("text-background");
  return (
    <Button
      disabled={disabled ?? false}
      type={type ?? "button"}
      variant={primary ? "primary" : "tertiary"}
      className={cn(
        "h-auto min-h-10 max-w-full min-w-0 whitespace-normal",
        layoutClasses(className),
      )}
      render={(nativeProps) => (
        <button
          type="button"
          {...mergeProps(nativeProps, definedProps(props))}
          ref={mergeRefs(nativeProps.ref, ref)}
          className={nativeProps.className}
        />
      )}
    >
      {children}
    </Button>
  );
}

export function ElementInput({ className, ...props }: ComponentPropsWithRef<"input">) {
  return (
    <Input
      {...definedProps(props)}
      className={cn("max-w-full min-w-0", layoutClasses(className))}
    />
  );
}

export function ElementTextArea({ className, ...props }: ComponentPropsWithRef<"textarea">) {
  return (
    <TextArea
      {...definedProps(props)}
      className={cn("max-w-full min-w-0", layoutClasses(className))}
    />
  );
}
