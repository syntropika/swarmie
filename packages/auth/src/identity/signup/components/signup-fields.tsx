import { CreatePasswordField, EmailField } from "~/src/identity/shared/components/auth-fields";
import { AuthForm, AuthSubmit } from "~/src/identity/shared/components/auth-form";
import { Feedback } from "~/src/identity/shared/components/feedback";
import type { SignupFieldsProps } from "~/src/identity/signup/types";
import { useLocale } from "~/src/platform/i18n/provider";

export function SignupFields(props: SignupFieldsProps) {
  const { t } = useLocale();
  return (
    <AuthForm pending={props.pending} onSubmit={props.onSubmit}>
      <EmailField value={props.email} disabled={props.pending} onChange={props.onEmailChange} />
      {props.method === "password" ? (
        <CreatePasswordField
          value={props.password}
          disabled={props.pending}
          onChange={props.onPasswordChange}
          visible={props.showPassword}
          onToggleVisibility={props.onTogglePassword}
        />
      ) : null}
      <AuthSubmit
        className="mt-0.5"
        label={props.method === "password" ? t("signup.submit") : t("magic.send")}
        pending={props.pending}
        loadingLabel={props.method === "password" ? t("signup.submitting") : t("magic.sending")}
      />
      <Feedback feedback={props.feedback} />
      <p className="text-notice leading-relaxed text-text-muted">{t("signup.passkeyHint")}</p>
    </AuthForm>
  );
}
