import { cn } from "../classes";
import { Button } from "./button";
import type { ButtonProps } from "./button";

export type ChromeButtonProps = Omit<ButtonProps, "size" | "variant" | "isIconOnly" | "type"> & {
  readonly size?:
    | "application"
    | "rail"
    | "tabClose"
    | "tab"
    | "navigation"
    | "navigationHeader"
    | "category"
    | "action"
    | "composer"
    | "composerCap"
    | "composerSend"
    | "panelSwitch";
  readonly selected?: boolean;
  readonly tone?: "neutral" | "brand";
};

/** Compact application chrome shares the vendor's press, disabled and keyboard-focus behavior. */
export function ChromeButton({
  size = "application",
  selected = false,
  tone = "neutral",
  className,
  render,
  ...props
}: ChromeButtonProps) {
  const geometry = {
    panelSwitch:
      "h-(--spacing-panel-switch) min-h-(--spacing-panel-switch) min-w-0 flex-1 justify-center gap-1.75 rounded-none p-0 text-(length:--text-panel-switch) [&_svg]:size-(--spacing-chrome-icon)",
    application:
      "h-(--spacing-chrome-action-height) min-h-(--spacing-chrome-action-height) w-(--spacing-chrome-action-width) min-w-(--spacing-chrome-action-width) rounded-small p-0 [&_svg]:size-(--spacing-chrome-icon)",
    rail: "size-(--spacing-chrome-rail-control) min-h-(--spacing-chrome-rail-control) min-w-(--spacing-chrome-rail-control) rounded-md p-0 [&_svg]:size-(--spacing-chrome-icon)",
    tabClose:
      "size-(--spacing-chrome-tab-close) min-h-(--spacing-chrome-tab-close) min-w-(--spacing-chrome-tab-close) rounded-small p-0 [&_svg]:size-3",
    tab: "h-(--spacing-chrome-tab) min-h-(--spacing-chrome-tab) min-w-0 flex-1 justify-start gap-1.5 rounded-t-md rounded-b-none px-2 py-0 text-chrome [&_svg]:size-(--spacing-chrome-icon)",
    navigation:
      "h-auto min-h-(--spacing-chrome-navigation-row) w-full min-w-0 justify-start gap-1.75 rounded-md px-(--spacing-navigation-row-x) py-(--spacing-navigation-row-y) text-left text-navigation [&_svg]:size-(--spacing-chrome-icon)",
    navigationHeader:
      "h-(--spacing-chrome-row) min-h-(--spacing-chrome-row) min-w-0 justify-start gap-1 rounded-md px-1 py-0 text-chrome [&_svg]:size-(--spacing-chrome-icon)",
    category:
      "h-(--spacing-navigation-header) min-h-(--spacing-navigation-header) w-full min-w-0 justify-start gap-2.5 rounded-md px-2.5 py-0 text-caption [&_svg]:size-4",
    action:
      "h-(--spacing-chrome-row) min-h-(--spacing-chrome-row) min-w-0 justify-start gap-1.5 rounded-md px-2 py-0 text-caption [&_svg]:size-(--spacing-chrome-icon)",
    composer:
      "size-(--spacing-composer-control) min-h-(--spacing-composer-control) min-w-(--spacing-composer-control) rounded-md p-0 text-chrome [&_svg]:size-(--spacing-chrome-icon)",
    composerCap:
      "h-(--spacing-composer-control) min-h-(--spacing-composer-control) min-w-0 gap-1.5 rounded-(--radius-composer-control) px-1.5 py-0 text-chrome [&_svg]:size-(--spacing-chrome-icon)",
    composerSend:
      "size-(--spacing-composer-control) min-h-(--spacing-composer-control) min-w-(--spacing-composer-control) rounded-md bg-accent p-0 text-accent-foreground enabled:hover:bg-accent-hover data-[hovered=true]:bg-accent-hover data-[hovered=true]:text-accent-foreground data-[pressed=true]:bg-accent-hover [&_svg]:size-(--spacing-chrome-icon)",
  } satisfies Record<NonNullable<ChromeButtonProps["size"]>, string>;
  const active = selected || props["aria-current"] === "page" || props["aria-pressed"] === true;
  const selectedStyle = "bg-row-selected text-foreground";
  const iconOnly = ["application", "rail", "tabClose", "composer", "composerSend"].includes(size);
  let color: string | undefined = "bg-transparent text-muted";
  if (size === "navigationHeader") {
    color = "bg-transparent text-foreground";
  }
  if (active) {
    color = selectedStyle;
  }
  if (size === "composerSend") {
    color = undefined;
  }
  return (
    <Button
      {...props}
      role={size === "tab" ? "tab" : props.role}
      render={(nativeProps, state) => {
        const forwarded = {
          ...nativeProps,
          ...(size === "tab" ? { "aria-selected": selected } : {}),
        };
        return render ? render(forwarded, state) : <button {...forwarded} type="button" />;
      }}
      variant="tertiary"
      isIconOnly={iconOnly}
      className={cn(
        "shrink-0 font-normal shadow-none enabled:hover:bg-row-hover enabled:hover:text-foreground aria-expanded:bg-row-selected aria-expanded:text-foreground data-[hovered=true]:bg-row-hover data-[hovered=true]:text-foreground data-[pressed=true]:scale-100 data-[pressed=true]:bg-row-hover",
        geometry[size],
        color,
        tone === "brand" &&
          active &&
          "text-brand-accent enabled:hover:text-brand-accent data-[hovered=true]:text-brand-accent",
        className,
      )}
    />
  );
}
