import { useNavigate } from "~/src/host";
import { PasswordResetView } from "~/src/identity/password-reset/components/password-reset-view";
import { usePasswordReset } from "~/src/identity/password-reset/hooks/use-password-reset";
import { observeBackgroundTask } from "~/src/platform/background-task";

export function ResetPassword() {
  const reset = usePasswordReset();
  const navigate = useNavigate();
  return (
    <PasswordResetView
      {...reset}
      onBack={() => {
        observeBackgroundTask(navigate({ to: "/login" }));
      }}
    />
  );
}
