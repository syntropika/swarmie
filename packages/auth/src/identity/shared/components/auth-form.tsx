import { Button, cn } from "@swarmie/ui-web";
import type { ReactNode } from "react";

import { LoginIcon } from "~/src/identity/shared/components/icon";
import { useLocale } from "~/src/platform/i18n/provider";

type AuthFormProps = {
  pending: boolean;
  onSubmit: () => void;
  children: ReactNode;
  className?: string;
};

export function AuthForm({
  pending,
  onSubmit,
  children,
  className = "grid w-full gap-5.5",
}: AuthFormProps) {
  return (
    <form
      className={className}
      aria-busy={pending}
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      {children}
    </form>
  );
}

type AuthSubmitProps = {
  label: string;
  pending: boolean;
  loadingLabel?: string;
  disabled?: boolean;
  showArrow?: boolean;
  className?: string;
};

export function AuthSubmit({
  label,
  pending,
  loadingLabel,
  disabled = false,
  showArrow = true,
  className,
}: AuthSubmitProps) {
  const { t } = useLocale();
  return (
    <Button
      size="xl"
      className={cn("w-full [overflow-wrap:anywhere] whitespace-normal", className)}
      type="submit"
      disabled={disabled}
      loading={pending}
      loadingLabel={loadingLabel ?? t("common.signingIn")}
    >
      {label}
      {showArrow ? <LoginIcon name="arrow" /> : null}
    </Button>
  );
}
