import * as Match from "effect/Match";
import { useState } from "react";

import { useAuthentication } from "~/src/host";
import { LoginView } from "~/src/identity/login/components/login-view";
import { useLogin } from "~/src/identity/login/hooks/use-login";
import { ChallengeForm } from "~/src/identity/shared/components/challenge-form";
import { SecureSignIn } from "~/src/identity/shared/components/secure-sign-in";

/** Connect the authentication hook to controlled presentation without transport logic. */
export function LoginController({ onAuthenticated }: { onAuthenticated?: () => void } = {}) {
  const { client: api, siteUrl } = useAuthentication();

  const login = useLogin(onAuthenticated);
  const [showPassword, setShowPassword] = useState(false);
  const form = Match.value(login.step).pipe(
    Match.when({ kind: "sign-in" }, () => null),
    Match.when({ kind: "request-reset" }, () => (
      <ChallengeForm
        kind="request-reset"
        value={login.email}
        pending={login.pending}
        feedback={login.feedback}
        onChange={login.changeEmail}
        onSubmit={login.submit}
        onBack={() => {
          login.restart();
        }}
      />
    )),
    Match.when({ kind: "verify-email" }, () => (
      <ChallengeForm
        kind="verify-email"
        value={login.code}
        pending={login.pending}
        feedback={login.feedback}
        onChange={login.changeCode}
        onSubmit={login.submit}
        onBack={() => {
          login.restart();
        }}
      />
    )),
    Match.when({ kind: "additional-verification" }, () => (
      <SecureSignIn
        href={api.loginUrl}
        onBack={() => {
          login.restart();
        }}
      />
    )),
    Match.exhaustive,
  );

  return (
    <LoginView
      method={login.method}
      email={login.email}
      password={login.password}
      showPassword={showPassword}
      pending={login.pending}
      feedback={login.feedback}
      homeHref={siteUrl}
      passkeyAvailable={login.passkeyAvailable}
      onMethodChange={(method) => {
        setShowPassword(false);
        login.restart(method);
      }}
      onEmailChange={login.changeEmail}
      onPasswordChange={login.changePassword}
      onTogglePassword={() => {
        setShowPassword((current) => !current);
      }}
      onSubmit={login.submit}
      onForgotPassword={login.requestReset}
      formContent={form}
    />
  );
}
