import { Link } from "~/src/host";
import { LoginFields } from "~/src/identity/login/components/login-fields";
import type { LoginViewProps } from "~/src/identity/login/types";
import { AuthMethods } from "~/src/identity/shared/components/auth-methods";
import { AuthIntroduction, AuthPage, AuthPanel } from "~/src/identity/shared/components/auth-page";
import { EntryPaths } from "~/src/identity/shared/components/entry-paths";
import { useLocale } from "~/src/platform/i18n/provider";

const methods = [
  { key: "password", label: "login.password", detail: "login.passwordDetail", icon: "lock" },
  { key: "magic", label: "login.magic", detail: "login.magicDetail", icon: "mail" },
  { key: "passkey", label: "login.passkey", detail: "login.passkeyDetail", icon: "key" },
] as const;

const introductions = {
  password: { title: "login.passwordTitle", description: "login.passwordDescription" },
  magic: { title: "login.magicTitle", description: "login.magicDescription" },
} as const;

function LoginIntroduction(props: Pick<LoginViewProps, "method" | "formContent" | "introduction">) {
  const { t } = useLocale();
  if (props.method === "passkey") {
    return null;
  }
  if (props.formContent !== undefined && props.formContent !== null && !props.introduction) {
    return null;
  }
  const introduction = props.introduction ?? {
    title: t(introductions[props.method].title),
    description: t(introductions[props.method].description),
  };
  return <AuthIntroduction {...introduction} />;
}

/** The approved Swarmie login composition, independent of any identity provider or transport. */
export function LoginView(props: LoginViewProps) {
  const { t } = useLocale();
  return (
    <AuthPage homeHref={props.homeHref}>
      <AuthMethods
        title={
          <>
            {t("login.space")}
            <br className="max-[760px]:hidden" />{" "}
            <span className="text-primary">{t("login.way")}</span>
          </>
        }
        description={t("login.choose")}
        label={t("login.method")}
        options={methods.map((method) => ({
          ...method,
          label: t(method.label),
          detail: t(method.detail),
        }))}
        columns="three"
        method={props.method}
        disabled={props.pending}
        onMethodChange={props.onMethodChange}
      />
      <AuthPanel label={t("login.signIn")} illustration={<EntryPaths method={props.method} />}>
        <LoginIntroduction {...props} />
        {props.formContent ?? <LoginFields {...props} />}
        <div className="mt-8 grid gap-3 pt-6 text-notice leading-relaxed text-ink max-[760px]:mt-[22px] max-[760px]:pt-4.5">
          <p>
            {t("signup.noAccount")}{" "}
            <Link
              className="inline-flex min-h-11 items-center font-medium text-primary hover:underline"
              to="/signup"
            >
              {t("signup.createAccount")}
            </Link>
          </p>
          <p>
            {t("login.joining")}
            <br />
            <span className="text-text-muted">{t("login.invitationEmail")}</span>
          </p>
        </div>
      </AuthPanel>
    </AuthPage>
  );
}
