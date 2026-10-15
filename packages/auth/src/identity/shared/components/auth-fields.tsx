import { Button, Field, Input } from "@swarmie/ui-web";
import { useId } from "react";
import type { ReactNode } from "react";

import { LoginIcon } from "~/src/identity/shared/components/icon";
import { useLocale } from "~/src/platform/i18n/provider";

type ControlledFieldProps = {
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
};

export function EmailField({ value, disabled, onChange }: ControlledFieldProps) {
  const { t } = useLocale();
  const id = useId();
  return (
    <Field id={id} label={t("field.email")} labelSize="compact">
      {(control) => (
        <Input
          {...control}
          controlSize="large"
          name="email"
          type="email"
          autoComplete="username"
          placeholder={t("field.emailPlaceholder")}
          required
          value={value}
          disabled={disabled}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      )}
    </Field>
  );
}

type PasswordFieldProps = {
  visible: boolean;
  onToggleVisibility: () => void;
  onForgotPassword: () => void;
} & ControlledFieldProps;

type PasswordEntryProps = {
  autoComplete: "current-password" | "new-password";
  labelAction?: ReactNode;
  description?: string;
} & Omit<PasswordFieldProps, "onForgotPassword">;

function PasswordEntry(props: PasswordEntryProps) {
  const { t } = useLocale();
  const id = useId();
  return (
    <Field
      id={id}
      label={t("field.password")}
      labelSize="compact"
      labelAction={props.labelAction}
      helper={props.description}
    >
      {(control) => (
        <div className="relative">
          <Input
            {...control}
            controlSize="large"
            adornment="trailing"
            name="password"
            type={props.visible ? "text" : "password"}
            autoComplete={props.autoComplete}
            placeholder={t("field.passwordPlaceholder")}
            required
            minLength={1}
            value={props.value}
            disabled={props.disabled}
            onChange={(event) => {
              props.onChange(event.target.value);
            }}
          />
          <Button
            variant="ghost"
            size="icon"
            className="absolute top-1/2 right-1 -translate-y-1/2"
            aria-label={props.visible ? t("field.hidePassword") : t("field.showPassword")}
            aria-pressed={props.visible}
            disabled={props.disabled}
            onClick={props.onToggleVisibility}
          >
            <LoginIcon name={props.visible ? "eye-off" : "eye"} />
          </Button>
        </div>
      )}
    </Field>
  );
}

export function PasswordField({ onForgotPassword, ...props }: PasswordFieldProps) {
  const { t } = useLocale();
  return (
    <PasswordEntry
      {...props}
      autoComplete="current-password"
      labelAction={
        <Button
          variant="link"
          size="sm"
          className="-my-2 whitespace-normal"
          disabled={props.disabled}
          onClick={onForgotPassword}
        >
          {t("field.forgot")}
        </Button>
      }
    />
  );
}

export function CreatePasswordField(props: Omit<PasswordFieldProps, "onForgotPassword">) {
  const { t } = useLocale();
  return (
    <PasswordEntry {...props} autoComplete="new-password" description={t("signup.passwordHelp")} />
  );
}

export function NewPasswordField({ value, disabled, onChange }: ControlledFieldProps) {
  const { t } = useLocale();
  const id = useId();
  return (
    <Field id={id} label={t("field.newPassword")} labelSize="compact">
      {(control) => (
        <Input
          {...control}
          controlSize="large"
          name="password"
          type="password"
          autoComplete="new-password"
          required
          value={value}
          disabled={disabled}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      )}
    </Field>
  );
}

export function VerificationCodeField({ value, disabled, onChange }: ControlledFieldProps) {
  const { t } = useLocale();
  const id = useId();
  return (
    <Field id={id} label={t("field.verificationCode")} labelSize="compact">
      {(control) => (
        <Input
          {...control}
          controlSize="large"
          name="code"
          inputMode="numeric"
          autoComplete="one-time-code"
          pattern="[0-9]{6}"
          maxLength={6}
          required
          value={value}
          disabled={disabled}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      )}
    </Field>
  );
}
