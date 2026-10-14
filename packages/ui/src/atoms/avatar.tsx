import type { ComponentPropsWithoutRef } from "react";

import { cn } from "../classes";

export type AvatarProps = Omit<ComponentPropsWithoutRef<"span">, "children"> & {
  kind: "person" | "agent" | "workspace";
  initials: string;
  label: string;
  size?: "sm" | "md";
};

const avatarAppearance = {
  person: "rounded-full bg-peach",
  agent: "rounded-pill bg-lilac",
  workspace: "rounded-pill bg-mint",
} as const;

/** Pair Agent avatars with a visible Agent label in the surrounding participant identity. */
export function Avatar({ kind, initials, label, size = "md", className, ...props }: AvatarProps) {
  return (
    <span
      {...props}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- This labeled composite graphic has no external image resource; ARIA exposes its parts as one image.
      role="img"
      aria-label={kind === "agent" ? `${label}, Agent` : label}
      className={cn(
        "inline-flex shrink-0 items-center justify-center font-medium text-ink",
        size === "sm" ? "size-8 text-xs" : "size-10 text-ui",
        avatarAppearance[kind],
        className,
      )}
    >
      {initials}
    </span>
  );
}
