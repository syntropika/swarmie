import * as Match from "effect/Match";
import { useState } from "react";

import { useAuthentication } from "~/src/host";
import { AuthIntroduction } from "~/src/identity/shared/components/auth-page";
import { ChallengeForm } from "~/src/identity/shared/components/challenge-form";
import { Feedback } from "~/src/identity/shared/components/feedback";
import { SecureSignIn } from "~/src/identity/shared/components/secure-sign-in";
import { SignupFields } from "~/src/identity/signup/components/signup-fields";
import { SignupView } from "~/src/identity/signup/components/signup-view";
import { useSignup } from "~/src/identity/signup/hooks/use-signup";
import { useLocale } from "~/src/platform/i18n/provider";

const introductions = {
  password: { title: "signup.passwordTitle", description: "signup.passwordDescription" },
  magic: { title: "signup.magicTitle", description: "signup.magicDescription" },
} as const;

/** Compose each explicit signup step without coupling the shared identity layout to its workflow. */
export function SignupController() {
  const { client: api, siteUrl } = useAuthentication();

  const signup = useSignup();
  const { t } = useLocale();
  const [showPassword, setShowPassword] = useState(false);
  const introduction = introductions[signup.method];
  const form = Match.value(signup.step).pipe(
    Match.when({ kind: "sign-up" }, () => (
      <>
        <AuthIntroduction title={t(introduction.title)} description={t(introduction.description)} />
        <SignupFields
          method={signup.method}
          email={signup.email}
          password={signup.password}
          showPassword={showPassword}
          pending={signup.pending}
          feedback={signup.feedback}
          onEmailChange={signup.changeEmail}
          onPasswordChange={signup.changePassword}
          onTogglePassword={() => {
            setShowPassword((current) => !current);
          }}
          onSubmit={signup.submit}
        />
      </>
    )),
    Match.when({ kind: "verify-email" }, () => (
      <>
        <ChallengeForm
          kind="verify-email"
          value={signup.code}
          pending={signup.pending}
          feedback={signup.feedback}
          onChange={signup.changeCode}
          onSubmit={signup.submit}
          onBack={signup.signIn}
        />
        <p className="mt-3 text-notice leading-relaxed text-text-muted">
          {t("signup.verificationHint")}
        </p>
      </>
    )),
    Match.when({ kind: "verification-recovery" }, () => (
      <>
        <AuthIntroduction
          title={t("signup.verificationRecoveryTitle")}
          description={t("signup.verificationRecovery")}
        />
        <Feedback feedback={signup.feedback} />
      </>
    )),
    Match.when({ kind: "additional-verification" }, () => (
      <SecureSignIn href={api.loginUrl} onBack={signup.signIn} />
    )),
    Match.exhaustive,
  );

  return (
    <SignupView
      homeHref={siteUrl}
      method={signup.method}
      methodsDisabled={signup.pending || signup.step.kind !== "sign-up"}
      onMethodChange={(method) => {
        setShowPassword(false);
        signup.changeMethod(method);
      }}
    >
      {form}
    </SignupView>
  );
}
