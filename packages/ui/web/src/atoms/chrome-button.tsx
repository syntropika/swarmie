import type { ComponentPropsWithRef } from "react";

import { cn } from "../classes";
import { Button } from "./button";
import type { ButtonProps } from "./button";

export type ChromeButtonProps = Omit<ButtonProps, "size" | "variant" | "isIconOnly" | "type"> &
  Pick<
    ComponentPropsWithRef<"button">,
    "role" | "title" | "tabIndex" | "popoverTarget" | "aria-busy"
  > & {
    readonly size?:
      | "application"
      | "rail"
      | "tabClose"
      | "tab"
      | "navigation"
      | "navigationMenu"
      | "project"
      | "navigationHeader"
      | "category"
      | "action"
      | "composer"
      | "composerAdd"
      | "composerCap"
      | "composerModel"
      | "composerSend"
      | "panelSwitch";
    readonly selected?: boolean;
    readonly appearance?: "default" | "bare";
  };

/** Application chrome supplies layout while the stock button owns appearance and interaction. */
export function ChromeButton({
  size = "application",
  selected = false,
  appearance = "default",
  className,
  render,
  role,
  title,
  tabIndex,
  popoverTarget,
  "aria-busy": busy,
  ...props
}: ChromeButtonProps) {
  const composerIcon =
    "size-(--spacing-composer-control) min-h-(--spacing-composer-control) min-w-(--spacing-composer-control) p-0 [&>svg]:size-(--spacing-chrome-icon)";
  const geometry = {
    panelSwitch:
      "h-(--spacing-panel-switch) min-h-(--spacing-panel-switch) min-w-0 flex-1 justify-center gap-1.75 p-0 [&>svg]:size-(--spacing-chrome-icon)",
    application:
      "h-(--spacing-chrome-action-height) min-h-(--spacing-chrome-action-height) w-(--spacing-chrome-action-width) min-w-(--spacing-chrome-action-width) p-0 [&>svg]:size-(--spacing-chrome-icon)",
    rail: "size-(--spacing-chrome-rail-control) min-h-(--spacing-chrome-rail-control) min-w-(--spacing-chrome-rail-control) p-0 [&>svg]:size-(--spacing-chrome-rail-icon)",
    tabClose:
      "size-(--spacing-chrome-tab-close) min-h-(--spacing-chrome-tab-close) min-w-(--spacing-chrome-tab-close) p-0 [&>svg]:size-3",
    tab: "h-(--spacing-chrome-tab) min-h-(--spacing-chrome-tab) min-w-0 flex-1 justify-start gap-1.5 px-2 py-0 [&>svg]:size-(--spacing-chrome-icon)",
    navigation:
      "h-auto min-h-(--spacing-chrome-navigation-row) w-full min-w-0 justify-start gap-1.75 px-(--spacing-navigation-row-x) py-(--spacing-navigation-row-y) text-left [&>svg]:size-(--spacing-chrome-icon)",
    navigationMenu:
      "h-(--spacing-menu-row) min-h-(--spacing-menu-row) w-full min-w-0 justify-start gap-(--spacing-menu-row-gap) px-(--spacing-menu-row-x) py-(--spacing-menu-row-y) text-left [&>svg]:size-(--spacing-menu-icon)",
    project:
      "h-(--spacing-chrome-row) min-h-(--spacing-chrome-row) w-full min-w-0 justify-start gap-1.75 px-(--spacing-navigation-row-x) py-0 text-left [&>svg]:size-(--spacing-chrome-icon)",
    navigationHeader:
      "h-(--spacing-chrome-row) min-h-(--spacing-chrome-row) min-w-0 justify-start gap-1 px-1 py-0 [&>svg]:size-(--spacing-chrome-icon)",
    category:
      "h-(--spacing-navigation-header) min-h-(--spacing-navigation-header) w-full min-w-0 justify-start gap-2.5 px-2.5 py-0 [&>svg]:size-4",
    action:
      "h-(--spacing-chrome-row) min-h-(--spacing-chrome-row) min-w-0 justify-start gap-1.5 px-2 py-0 [&>svg]:size-(--spacing-chrome-icon)",
    composer: composerIcon,
    composerAdd: composerIcon,
    composerCap:
      "h-(--spacing-composer-control) min-h-(--spacing-composer-control) min-w-0 gap-(--spacing-composer-cap-gap) px-(--spacing-composer-cap-x) py-0 [&>svg]:size-(--spacing-chrome-icon)",
    composerModel: `${composerIcon} justify-center gap-0`,
    composerSend: `${composerIcon} [--button-bg:var(--color-brand-accent)] [--button-bg-hover:var(--accent-hover)] [--button-bg-pressed:var(--accent-hover)] [--button-fg:var(--color-brand-accent-foreground)]`,
  } satisfies Record<NonNullable<ChromeButtonProps["size"]>, string>;
  const active =
    selected ||
    props["aria-current"] === "page" ||
    props["aria-pressed"] === true ||
    props["aria-pressed"] === "true";
  const iconOnly = [
    "application",
    "rail",
    "tabClose",
    "composer",
    "composerAdd",
    "composerModel",
    "composerSend",
  ].includes(size);
  const classes = cn(
    "shrink-0",
    geometry[size],
    appearance !== "bare" &&
      size !== "composerSend" &&
      (active
        ? "[--button-bg-hover:var(--color-row-selected-hover)] [--button-bg-pressed:var(--color-row-selected-hover)] [--button-bg:var(--color-row-selected)]"
        : "[--button-bg-hover:var(--color-row-hover)] [--button-bg-pressed:var(--color-row-hover)]"),
  );
  let variant: ButtonProps["variant"] = active && appearance !== "bare" ? "secondary" : "ghost";
  if (size === "composerSend") {
    variant = "primary";
  }
  return (
    <Button
      {...props}
      render={(nativeProps, state) => {
        const forwarded = {
          ...nativeProps,
          "data-chrome": size,
          role: size === "tab" ? "tab" : (role ?? nativeProps.role),
          title: title ?? nativeProps.title,
          tabIndex: tabIndex ?? nativeProps.tabIndex,
          popoverTarget: popoverTarget ?? nativeProps.popoverTarget,
          "aria-busy": busy ?? nativeProps["aria-busy"],
          ...(size === "tab" ? { "aria-selected": selected } : {}),
        };
        return render ? render(forwarded, state) : <button {...forwarded} type="button" />;
      }}
      variant={variant}
      isIconOnly={iconOnly}
      className={
        typeof className === "function"
          ? (state) => cn(classes, className(state))
          : cn(classes, className)
      }
    />
  );
}
