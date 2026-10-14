import type { ComponentPropsWithRef, ReactNode } from "react";

import { cn } from "../classes";

export type FieldControlProps = {
  id: string;
  "aria-describedby": string | undefined;
  "aria-invalid": true | undefined;
};

export type FieldProps = Omit<ComponentPropsWithRef<"div">, "children" | "id"> & {
  id: string;
  label: string;
  labelSize?: "normal" | "compact";
  /** Keep a persistent accessible label when a compact control does not need a visible heading. */
  labelVisibility?: "visible" | "screen-reader";
  labelAction?: ReactNode;
  helper?: ReactNode;
  error?: ReactNode;
  children: (control: FieldControlProps) => ReactNode;
};

/** Spread the provided control props on one input, textarea, or select to link its label and descriptions. */
export function Field({
  id,
  label,
  labelSize = "normal",
  labelVisibility = "visible",
  labelAction,
  helper,
  error,
  children,
  className,
  ...props
}: FieldProps) {
  const hasHelper = Boolean(helper);
  const hasError = Boolean(error);
  const descriptions = [hasHelper ? `${id}-helper` : null, hasError ? `${id}-error` : null]
    .filter(Boolean)
    .join(" ");
  const fieldLabel = (
    <label
      htmlFor={id}
      className={
        labelVisibility === "screen-reader"
          ? "sr-only"
          : cn(
              "min-w-0 leading-ui text-ink",
              labelSize === "compact" ? "text-notice font-medium" : "text-ui font-medium",
            )
      }
    >
      {label}
    </label>
  );

  return (
    <div {...props} className={cn("flex flex-col gap-2", className)}>
      {labelVisibility === "visible" || Boolean(labelAction) ? (
        <div
          className={cn(
            "flex min-h-5 items-center gap-3",
            labelVisibility === "screen-reader" ? "justify-end" : "justify-between",
          )}
        >
          {fieldLabel}
          {labelAction}
        </div>
      ) : (
        fieldLabel
      )}
      {children({
        id,
        "aria-describedby": descriptions || undefined,
        "aria-invalid": hasError || undefined,
      })}
      {hasHelper ? (
        <p id={`${id}-helper`} className="text-ui leading-relaxed text-text-muted">
          {helper}
        </p>
      ) : null}
      {hasError ? (
        <p id={`${id}-error`} className="text-ui leading-relaxed text-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}
