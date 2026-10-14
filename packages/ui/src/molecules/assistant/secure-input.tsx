import { Fragment, useId, useRef, useState } from "react";
import type { ComponentPropsWithRef, ReactNode } from "react";

import { Badge } from "../../atoms/badge";
import { Button } from "../../atoms/button";
import { Icon } from "../../atoms/icon";
import { Input } from "../../atoms/input";
import { cn } from "../../classes";
import { TooltipIconButton } from "./shared";

/** The host owns submission and retains or clears sensitive values at its workflow boundary. */
export type SecureInputValues = Readonly<Record<string, string>>;

/** Actual lifecycle supplied by the host; this component never simulates transmission. */
export type SecureInputState = "form" | "sending" | "filled" | "failed" | "dismissed";

export type SecureInputPreset = "secret" | "login" | "otp" | "card" | "pin";

type PresetFields =
  | { preset: "secret" | "otp"; fieldLabels: { code: string } }
  | { preset: "pin"; fieldLabels: { pin: string } }
  | { preset: "login"; fieldLabels: { email: string; password: string } }
  | { preset: "card"; fieldLabels: { number: string; expiry: string; cvc: string } };

export type SecureInputLabels = {
  continue: string;
  dismiss: string;
  openScreen: string;
  showPassword: string;
  hidePassword: string;
  digit: (position: number, count: number) => string;
  expiryPlaceholder: string;
  statuses: Record<Exclude<SecureInputState, "form">, { label: string; description: ReactNode }>;
};

export type SecureInputProps = Omit<ComponentPropsWithRef<"form">, "onSubmit" | "title"> &
  PresetFields & {
    disabled?: boolean;
    title: ReactNode;
    description: ReactNode;
    values: SecureInputValues;
    onValueChange: (values: SecureInputValues) => void;
    onContinue: (values: SecureInputValues) => void;
    onDismiss: () => void;
    onOpenScreen?: () => void;
    state: SecureInputState;
    labels: SecureInputLabels;
  };

function digits(value: string, count: number) {
  return value.replaceAll(/\D/gu, "").slice(0, count);
}

function cardReady(values: SecureInputValues) {
  const expiry = /^(?<month>\d{2})\/(?<year>\d{2})$/u.exec(values.expiry ?? "");
  const month = Number(expiry?.groups?.month);
  return (
    digits(values.number ?? "", 16).length === 16 &&
    expiry !== null &&
    month >= 1 &&
    month <= 12 &&
    digits(values.cvc ?? "", 4).length >= 3
  );
}

function ready(preset: SecureInputPreset, values: SecureInputValues) {
  switch (preset) {
    case "secret": {
      return (values.code ?? "").trim().length > 0;
    }
    case "login": {
      return (
        /^\S+@\S+\.\S+$/u.test((values.email ?? "").trim()) && (values.password ?? "").length > 0
      );
    }
    case "otp": {
      return digits(values.code ?? "", 6).length === 6;
    }
    case "pin": {
      return digits(values.pin ?? "", 4).length === 4;
    }
    case "card": {
      return cardReady(values);
    }
    default: {
      return false;
    }
  }
}

/** Segmented code entry supports paste, autofill, arrows, deletion, and automatic focus advance. */
function CodeCells({
  length,
  label,
  value,
  onChange,
  digitLabel,
  disabled,
}: {
  length: 4 | 6;
  label: string;
  value: string;
  onChange: (value: string) => void;
  digitLabel: SecureInputLabels["digit"];
  disabled: boolean;
}) {
  const id = useId();
  const refs = useRef<(HTMLInputElement | null)[]>([]);
  const positions = length === 6 ? [1, 2, 3, 4, 5, 6] : [1, 2, 3, 4];
  const write = (next: string, focus: number) => {
    onChange(next.slice(0, length));
    refs.current[Math.max(0, Math.min(focus, length - 1))]?.focus();
  };
  return (
    <fieldset disabled={disabled} className="min-w-0 space-y-2">
      <legend id={id} className="text-ui font-medium">
        {label}
        <span aria-hidden="true"> *</span>
      </legend>
      <div
        aria-labelledby={id}
        className="flex items-center gap-1.5"
        onPaste={(event) => {
          const text = digits(event.clipboardData.getData("text"), length);
          if (!text) {
            return;
          }
          event.preventDefault();
          write(text, text.length - 1);
        }}
      >
        {positions.map((position) => {
          const index = position - 1;
          return (
            <Fragment key={position}>
              {length === 6 && position === 4 ? <span aria-hidden="true" className="w-2" /> : null}
              <Input
                focusFeedback="ring"
                ref={(node) => {
                  refs.current[index] = node;
                }}
                controlSize="compact"
                className="size-control shrink-0 rounded-lg p-0 text-center font-medium tabular-nums pointer-coarse:size-control-touch"
                inputMode="numeric"
                autoComplete={index === 0 ? "one-time-code" : "off"}
                maxLength={length}
                value={value[index] ?? ""}
                required
                aria-label={digitLabel(position, length)}
                onFocus={(event) => {
                  event.currentTarget.select();
                }}
                onChange={(event) => {
                  const entered = digits(event.target.value, length);
                  if (!entered) {
                    write(value.slice(0, index) + value.slice(index + 1), index);
                    return;
                  }
                  write(
                    value.slice(0, index) + entered + value.slice(index + entered.length),
                    index + entered.length,
                  );
                }}
                onKeyDown={(event) => {
                  if (event.key === "Backspace") {
                    event.preventDefault();
                    if ((value[index] ?? "").length > 0) {
                      write(value.slice(0, index) + value.slice(index + 1), index);
                    } else if (index > 0) {
                      write(value.slice(0, index - 1) + value.slice(index), index - 1);
                    }
                  } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
                    event.preventDefault();
                    refs.current[
                      Math.max(
                        0,
                        Math.min(length - 1, index + (event.key === "ArrowRight" ? 1 : -1)),
                      )
                    ]?.focus();
                  }
                }}
              />
            </Fragment>
          );
        })}
      </div>
    </fieldset>
  );
}

function SecureInputStatus({
  title,
  state,
  labels,
  className,
}: {
  title: ReactNode;
  state: Exclude<SecureInputState, "form">;
  labels: SecureInputLabels;
  className: string | undefined;
}) {
  const status = labels.statuses[state];
  return (
    <section
      aria-live="polite"
      aria-busy={state === "sending"}
      className={cn(
        "flex flex-wrap items-start justify-between gap-3 rounded-panel bg-surface p-4",
        className,
      )}
    >
      <div className="min-w-0 flex-1 space-y-1">
        <p className="text-ui font-medium">{title}</p>
        <p className="text-ui text-text-muted">{status.description}</p>
      </div>
      <Badge tone={state === "failed" ? "danger" : "neutral"}>
        {state === "sending" ? (
          <Icon name="loading" className="size-3.5 motion-safe:animate-assistant-spin" />
        ) : null}
        {status.label}
      </Badge>
    </section>
  );
}

/** Five source presets, with controlled values and lifecycle, persistent labels, and no invented result. */
export function SecureInput({
  title,
  description,
  values,
  onValueChange,
  onContinue,
  onDismiss,
  onOpenScreen,
  state,
  labels,
  className,
  disabled = false,
  ...rest
}: SecureInputProps) {
  const { preset, fieldLabels, ...nativeProps } = rest;
  const id = useId();
  const [revealed, setRevealed] = useState(false);
  const set = (name: string, value: string) => {
    onValueChange({ ...values, [name]: value });
  };
  const field = (name: string, label: string, props: ComponentPropsWithRef<typeof Input>) => (
    <label className="flex min-w-0 flex-col gap-2" htmlFor={`${id}-${name}`}>
      <span className="text-ui font-medium">
        {label}
        <span aria-hidden="true"> *</span>
      </span>
      <Input
        focusFeedback="ring"
        {...props}
        id={`${id}-${name}`}
        required
        value={values[name] ?? ""}
        onChange={(event) => {
          set(name, event.target.value);
        }}
      />
    </label>
  );
  if (state !== "form") {
    return <SecureInputStatus title={title} labels={labels} state={state} className={className} />;
  }
  let fields: ReactNode;
  switch (preset) {
    case "secret": {
      fields = field("code", fieldLabels.code, { autoComplete: "one-time-code" });
      break;
    }
    case "otp": {
      fields = (
        <CodeCells
          length={6}
          label={fieldLabels.code}
          value={values.code ?? ""}
          onChange={(value) => {
            set("code", value);
          }}
          digitLabel={labels.digit}
          disabled={disabled}
        />
      );
      break;
    }
    case "pin": {
      fields = (
        <CodeCells
          length={4}
          label={fieldLabels.pin}
          value={values.pin ?? ""}
          onChange={(value) => {
            set("pin", value);
          }}
          digitLabel={labels.digit}
          disabled={disabled}
        />
      );
      break;
    }
    case "login": {
      fields = (
        <>
          {field("email", fieldLabels.email, {
            type: "email",
            inputMode: "email",
            autoComplete: "username",
            autoCapitalize: "off",
            spellCheck: false,
          })}
          <div className="flex min-w-0 flex-col gap-2">
            <label className="text-ui font-medium" htmlFor={`${id}-password`}>
              {fieldLabels.password}
              <span aria-hidden="true"> *</span>
            </label>
            <span className="relative">
              <Input
                focusFeedback="ring"
                id={`${id}-password`}
                type={revealed ? "text" : "password"}
                required
                autoComplete="current-password"
                adornment="trailing"
                value={values.password ?? ""}
                onChange={(event) => {
                  set("password", event.target.value);
                }}
              />
              <Button
                size="icon-sm"
                variant="ghost"
                className="absolute top-1/2 right-1 -translate-y-1/2"
                aria-label={revealed ? labels.hidePassword : labels.showPassword}
                aria-pressed={revealed}
                onClick={() => {
                  setRevealed((current) => !current);
                }}
              >
                <Icon name={revealed ? "eye-off" : "eye"} className="size-4" />
              </Button>
            </span>
          </div>
        </>
      );
      break;
    }
    case "card": {
      fields = (
        <>
          <label className="flex min-w-0 flex-col gap-2" htmlFor={`${id}-number`}>
            <span className="text-ui font-medium">
              {fieldLabels.number}
              <span aria-hidden="true"> *</span>
            </span>
            <Input
              focusFeedback="ring"
              id={`${id}-number`}
              inputMode="numeric"
              autoComplete="cc-number"
              required
              value={values.number ?? ""}
              onChange={(event) => {
                set(
                  "number",
                  digits(event.target.value, 16).replaceAll(/(?<group>\d{4})(?=\d)/gu, "$<group> "),
                );
              }}
            />
          </label>
          <div className="grid grid-cols-2 gap-3">
            <label className="flex min-w-0 flex-col gap-2" htmlFor={`${id}-expiry`}>
              <span className="text-ui font-medium">
                {fieldLabels.expiry}
                <span aria-hidden="true"> *</span>
              </span>
              <Input
                focusFeedback="ring"
                id={`${id}-expiry`}
                required
                inputMode="numeric"
                autoComplete="cc-exp"
                placeholder={labels.expiryPlaceholder}
                value={values.expiry ?? ""}
                onChange={(event) => {
                  const raw = digits(event.target.value, 4);
                  set("expiry", raw.length <= 2 ? raw : `${raw.slice(0, 2)}/${raw.slice(2)}`);
                }}
              />
            </label>
            <label className="flex min-w-0 flex-col gap-2" htmlFor={`${id}-cvc`}>
              <span className="text-ui font-medium">
                {fieldLabels.cvc}
                <span aria-hidden="true"> *</span>
              </span>
              <Input
                focusFeedback="ring"
                id={`${id}-cvc`}
                required
                inputMode="numeric"
                autoComplete="cc-csc"
                value={values.cvc ?? ""}
                onChange={(event) => {
                  set("cvc", digits(event.target.value, 4));
                }}
              />
            </label>
          </div>
        </>
      );
      break;
    }
    default: {
      fields = null;
    }
  }
  return (
    <form
      {...nativeProps}
      aria-labelledby={`${id}-title`}
      aria-describedby={`${id}-description`}
      className={cn(
        "space-y-4 rounded-panel bg-surface p-4 text-ui leading-ui text-ink",
        className,
      )}
      onSubmit={(event) => {
        event.preventDefault();
        if (disabled || !ready(preset, values)) {
          return;
        }
        onContinue(
          preset === "card"
            ? {
                ...values,
                number: digits(values.number ?? "", 16),
                cvc: digits(values.cvc ?? "", 4),
              }
            : values,
        );
      }}
    >
      <div className="space-y-1">
        <p id={`${id}-title`} className="font-medium">
          {title}
        </p>
        <p id={`${id}-description`} className="text-text-muted">
          {description}
        </p>
      </div>
      <fieldset disabled={disabled} className="min-w-0 space-y-3">
        {fields}
      </fieldset>
      <div className="flex flex-wrap items-center gap-2">
        <Button type="submit" size="sm" disabled={disabled || !ready(preset, values)}>
          {labels.continue}
        </Button>
        <Button size="sm" variant="ghost" disabled={disabled} onClick={onDismiss}>
          {labels.dismiss}
        </Button>
        {onOpenScreen ? (
          <TooltipIconButton
            label={labels.openScreen}
            icon="screen"
            disabled={disabled}
            onClick={onOpenScreen}
          />
        ) : null}
      </div>
    </form>
  );
}
