import { Slot } from "@radix-ui/react-slot";
import { cva } from "class-variance-authority";
import type { VariantProps } from "class-variance-authority";
import type { ComponentPropsWithRef, ReactElement } from "react";

import { cn } from "../classes";
import { Icon } from "./icon";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 rounded-pill border text-ui leading-ui font-medium whitespace-normal transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:border-transparent disabled:bg-surface-muted disabled:text-text-muted motion-reduce:transition-none",
  {
    variants: {
      variant: {
        choice: "border-transparent bg-surface-muted text-ink hover:bg-surface-hover",
        disclosure:
          "justify-start rounded-pill border-transparent bg-transparent text-left font-medium whitespace-normal text-ink hover:bg-surface-hover aria-expanded:bg-surface-hover",
        primary:
          "border-primary bg-primary text-on-primary hover:border-primary-hover hover:bg-primary-hover",
        secondary: "border-transparent bg-surface-muted text-ink hover:bg-surface-hover",
        ghost:
          "border-transparent bg-transparent text-ink hover:bg-surface-hover disabled:border-transparent disabled:bg-transparent",
        position:
          "border-transparent bg-transparent text-ink hover:bg-surface-hover focus-visible:outline-none",
        floating:
          "border-primary bg-primary text-on-primary hover:border-primary-hover hover:bg-primary-hover focus-visible:border-primary-hover focus-visible:bg-primary-hover focus-visible:outline-none",
        destructive: "border-danger bg-danger text-on-primary hover:bg-danger/90",
        link: "border-transparent bg-transparent text-primary hover:underline",
        brand:
          "border-0 bg-transparent text-right leading-ui whitespace-normal hover:bg-transparent hover:text-primary",
        navigation:
          "justify-start rounded-pill border-transparent bg-transparent text-left font-medium whitespace-normal text-ink hover:bg-surface-hover",
        quiet:
          "justify-start rounded-pill border-transparent bg-transparent text-left font-medium whitespace-normal text-ink hover:bg-surface-hover disabled:border-transparent disabled:bg-transparent",
      },
      size: {
        position: "h-3 min-h-3 w-11 justify-start rounded-pill p-0",
        inline:
          "min-h-6 border-0 px-1 py-0 align-baseline text-body leading-body pointer-coarse:min-h-control-touch",
        menu: "min-h-control-compact rounded-pill px-2 py-1 pointer-coarse:min-h-control-touch",
        panel: "min-h-control rounded-pill px-2.5 py-1.5 pointer-coarse:min-h-control-touch",
        sm: "min-h-control-small px-3 py-1 pointer-coarse:min-h-control-touch",
        md: "min-h-control px-4 py-1.5 pointer-coarse:min-h-control-touch",
        lg: "min-h-control-large px-4 py-2 text-body leading-body pointer-coarse:min-h-control-touch",
        xl: "min-h-control-entry px-4 py-2 text-body leading-body",
        icon: "size-control rounded-pill p-0 pointer-coarse:size-control-touch",
        "icon-sm": "size-control-compact rounded-pill p-0 pointer-coarse:size-control-touch",
        auth: "min-h-control-large gap-2 px-3 py-2 text-ui leading-ui pointer-coarse:min-h-control-touch",
        navigation:
          "min-h-control gap-2 rounded-pill py-1.5 pr-1.5 pl-2.5 pointer-coarse:min-h-control-touch",
        account: "min-h-control-entry gap-2 rounded-pill px-2 py-1.5",
        brand: "min-h-control gap-2 px-0 pointer-coarse:min-h-control-touch",
      },
      selectionTone: { neutral: "", mint: "", peach: "", lilac: "" },
      selected: { true: "", false: "" },
    },
    compoundVariants: [
      {
        variant: "choice",
        selected: true,
        selectionTone: "neutral",
        class: "bg-primary-soft text-ink hover:bg-primary-soft",
      },
      {
        variant: "choice",
        selected: true,
        selectionTone: "mint",
        class: "bg-mint text-ink hover:bg-mint",
      },
      {
        variant: "choice",
        selected: true,
        selectionTone: "peach",
        class: "bg-peach text-ink hover:bg-peach",
      },
      {
        variant: "choice",
        selected: true,
        selectionTone: "lilac",
        class: "bg-lilac text-ink hover:bg-lilac",
      },
      {
        variant: "ghost",
        selected: true,
        class: "bg-primary-soft text-primary hover:text-primary-hover",
      },
      {
        variant: "navigation",
        selected: true,
        class: "bg-primary-soft font-medium text-primary hover:text-primary-hover",
      },
    ],
    defaultVariants: { variant: "primary", size: "md", selectionTone: "neutral", selected: false },
  },
);

type ButtonAppearance = VariantProps<typeof buttonVariants>;

type NativeButtonProps = ComponentPropsWithRef<"button"> &
  ButtonAppearance & {
    asChild?: false;
    loading?: boolean;
    loadingLabel?: string;
  };

type ComposedButtonProps = Omit<ComponentPropsWithRef<typeof Slot>, "children"> &
  ButtonAppearance & {
    asChild: true;
    children: ReactElement;
    disabled?: never;
    loading?: never;
    loadingLabel?: never;
  };

export type ButtonProps = NativeButtonProps | ComposedButtonProps;

/**
 * Native actions default to type="button" and are disabled while loading.
 * Use asChild for one semantic link; disabled/loading states belong to native actions.
 */
export function Button(props: ButtonProps) {
  if (props.asChild === true) {
    const { asChild, variant, size, selectionTone, selected, className, ...linkProps } = props;

    return (
      <Slot
        className={cn(buttonVariants({ variant, size, selectionTone, selected }), className)}
        {...linkProps}
      />
    );
  }

  const {
    asChild,
    variant,
    size,
    selectionTone,
    selected,
    className,
    loading = false,
    loadingLabel = "Working",
    disabled = false,
    type = "button",
    children,
    ...buttonProps
  } = props;

  return (
    <button
      {...buttonProps}
      // oxlint-disable-next-line react/button-has-type -- Native button props constrain this value; the primitive defaults to a non-submitting button.
      type={type}
      disabled={disabled || loading}
      aria-busy={loading ? true : undefined}
      className={cn(buttonVariants({ variant, size, selectionTone, selected }), className)}
    >
      {loading ? <Icon name="loading" className="size-3.5 motion-safe:animate-spin" /> : null}
      {children}
      {loading ? <span className="sr-only">{loadingLabel}</span> : null}
    </button>
  );
}
