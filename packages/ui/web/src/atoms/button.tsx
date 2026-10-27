/* oxlint-disable react/button-has-type -- The vendor supplies the validated native button type; absent types default to button. */
import { Button as HeroButton, Spinner } from "@heroui/react";
import type { ButtonProps as HeroButtonProps } from "@heroui/react";
import type { ComponentPropsWithRef } from "react";

type Variant = Exclude<HeroButtonProps["variant"], "ghost">;
type ActionProps = Omit<HeroButtonProps, "variant"> & {
  role?: ComponentPropsWithRef<"button">["role"];
  "aria-busy"?: ComponentPropsWithRef<"button">["aria-busy"];
  title?: string | undefined;
  tabIndex?: number | undefined;
  popoverTarget?: string | undefined;
  variant?: Variant;
  disabled?: boolean | undefined;
  tone?: "neutral" | "brand";
  /** Bordered-panel actions use the shared field radius rather than a pill. */
  shape?: "pill" | "panel" | "tile";
  loading?: boolean;
  loadingLabel?: string;
};
export type ButtonProps = ActionProps;

/** HeroUI owns action behavior, sizing and focus; the wrapper adds announced loading state. */
function Root(props: ButtonProps) {
  const {
    children,
    disabled = false,
    isDisabled,
    tone = "neutral",
    shape = "pill",
    loading = false,
    isPending = false,
    loadingLabel = "Working",
    title,
    tabIndex,
    popoverTarget,
    role,
    render,
    ...rest
  } = props;
  const pending = loading || isPending;
  const effectiveDisabled = disabled || pending ? true : isDisabled;
  const disabledProps = effectiveDisabled === undefined ? {} : { isDisabled: effectiveDisabled };
  return (
    <HeroButton
      {...rest}
      render={(nativeProps, state) => {
        const forwarded = {
          ...nativeProps,
          "data-tone": tone === "brand" ? "brand" : undefined,
          "data-shape": shape === "pill" ? undefined : shape,
          type: nativeProps.type ?? "button",
          "aria-busy": pending ? true : rest["aria-busy"],
          title: title ?? nativeProps.title,
          tabIndex: tabIndex ?? nativeProps.tabIndex,
          role: role ?? nativeProps.role,
          popoverTarget: popoverTarget ?? nativeProps.popoverTarget,
        };
        return render ? render(forwarded, state) : <button {...forwarded} />;
      }}
      {...disabledProps}
      isPending={pending}
      aria-busy={pending ? true : rest["aria-busy"]}
    >
      {(state) => (
        <>
          {pending ? <Spinner size="sm" color="current" aria-hidden="true" /> : null}
          {typeof children === "function" ? children(state) : children}
          {pending ? <span className="sr-only">{loadingLabel}</span> : null}
        </>
      )}
    </HeroButton>
  );
}

// oxlint-disable-next-line no-redeclare -- The vendor API pairs a value with its compound prop type namespace.
export const Button = Object.assign(Root, { Root });
export { Root as ButtonRoot };
export type ButtonRootProps = ButtonProps;
export type Button = { Props: ButtonProps; RootProps: ButtonRootProps };
