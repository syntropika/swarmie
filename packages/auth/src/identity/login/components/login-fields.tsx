import type { LoginViewProps } from "~/src/identity/login/types";
import { EmailField, PasswordField } from "~/src/identity/shared/components/auth-fields";
import { AuthForm, AuthSubmit } from "~/src/identity/shared/components/auth-form";
import { Feedback } from "~/src/identity/shared/components/feedback";
import { useLocale } from "~/src/platform/i18n/provider";

type SignInFormProps = Pick<
  LoginViewProps,
  "email" | "pending" | "feedback" | "onEmailChange" | "onSubmit"
>;
type PasswordFormProps = SignInFormProps &
  Pick<
    LoginViewProps,
    "password" | "showPassword" | "onPasswordChange" | "onTogglePassword" | "onForgotPassword"
  >;
type PasskeyFormProps = SignInFormProps & Pick<LoginViewProps, "passkeyAvailable">;

function PasswordForm(props: PasswordFormProps) {
  const { t } = useLocale();
  return (
    <AuthForm pending={props.pending} onSubmit={props.onSubmit}>
      <EmailField value={props.email} disabled={props.pending} onChange={props.onEmailChange} />
      <PasswordField
        value={props.password}
        disabled={props.pending}
        onChange={props.onPasswordChange}
        visible={props.showPassword}
        onToggleVisibility={props.onTogglePassword}
        onForgotPassword={props.onForgotPassword}
      />
      <AuthSubmit className="mt-0.5" label={t("login.signIn")} pending={props.pending} />
      <Feedback feedback={props.feedback} />
    </AuthForm>
  );
}

function MagicLinkForm(props: SignInFormProps) {
  const { t } = useLocale();
  return (
    <AuthForm pending={props.pending} onSubmit={props.onSubmit}>
      <EmailField value={props.email} disabled={props.pending} onChange={props.onEmailChange} />
      <p className="text-ui leading-relaxed text-text-muted">{t("magic.helper")}</p>
      <AuthSubmit
        className="mt-0.5"
        label={t("magic.send")}
        pending={props.pending}
        loadingLabel={t("magic.sending")}
      />
      <Feedback feedback={props.feedback} />
    </AuthForm>
  );
}

function PasskeyForm(props: PasskeyFormProps) {
  const { t } = useLocale();
  const description =
    props.passkeyAvailable === true ? t("passkey.description") : t("passkey.unavailableSignIn");
  return (
    <AuthForm className="grid gap-5" pending={props.pending} onSubmit={props.onSubmit}>
      <h2 className="text-title font-medium text-balance">{t("passkey.signIn")}</h2>
      <p className="text-body leading-relaxed text-text-muted">{description}</p>
      <EmailField value={props.email} disabled={props.pending} onChange={props.onEmailChange} />
      <AuthSubmit
        label={t("passkey.continue")}
        pending={props.pending}
        disabled={props.passkeyAvailable !== true}
      />
      <Feedback feedback={props.feedback} />
    </AuthForm>
  );
}

const signInForms = { password: PasswordForm, magic: MagicLinkForm, passkey: PasskeyForm };

/** Controlled fields; the caller owns every authentication transition. */
export function LoginFields(props: LoginViewProps) {
  const Form = signInForms[props.method];
  return <Form {...props} />;
}
