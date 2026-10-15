import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";

export type AlertTone = "info" | "success" | "error";
export type AlertRootProps = ComponentPropsWithRef<"div"> & {
  tone: AlertTone;
  appearance?: "soft" | "plain";
  spacing?: "normal" | "compact" | "none";
};

const tones: Record<AlertTone, string> = {
  info: "bg-primary-soft text-ink",
  success: "bg-success-soft text-success",
  error: "bg-danger-soft text-danger",
};

/** Compose a notice from optional icon and content slots; the caller owns messages and actions. */
export function AlertRoot({
  tone,
  role,
  appearance = "soft",
  spacing = "normal",
  className,
  ...props
}: AlertRootProps) {
  const liveRole = role ?? (tone === "error" ? "alert" : "status");
  return (
    <div
      aria-atomic="true"
      {...props}
      role={liveRole}
      data-tone={tone}
      className={cn(
        "group/alert flex w-full items-start gap-3 rounded-control text-notice",
        spacing === "normal" ? "p-3" : null,
        spacing === "compact" ? "px-3 py-2" : null,
        tones[tone],
        appearance === "plain" ? "bg-transparent" : null,
        className,
      )}
    />
  );
}

export function AlertIcon({ className, ...props }: ComponentPropsWithRef<"span">) {
  return (
    <span
      {...props}
      aria-hidden="true"
      className={cn(
        "mt-0.75 flex size-5 shrink-0 group-data-[tone=info]/alert:text-primary",
        className,
      )}
    />
  );
}

export function AlertContent({ className, ...props }: ComponentPropsWithRef<"div">) {
  return <div {...props} className={cn("min-w-0 flex-1", className)} />;
}

export function AlertTitle({ className, ...props }: ComponentPropsWithRef<"strong">) {
  return <strong {...props} className={cn("block text-ui font-medium", className)} />;
}

export function AlertDescription({
  className,
  size = "default",
  ...props
}: ComponentPropsWithRef<"p"> & { size?: "default" | "body" }) {
  return (
    <p
      {...props}
      className={cn(
        "mt-1 wrap-anywhere",
        size === "body" ? "text-ui leading-body" : "leading-notice",
        className,
      )}
    />
  );
}
