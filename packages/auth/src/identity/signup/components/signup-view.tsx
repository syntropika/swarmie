import { Link } from "~/src/host";
import { AuthMethods } from "~/src/identity/shared/components/auth-methods";
import { AuthPage, AuthPanel } from "~/src/identity/shared/components/auth-page";
import { EntryPaths } from "~/src/identity/shared/components/entry-paths";
import type { SignupViewProps } from "~/src/identity/signup/types";
import { useLocale } from "~/src/platform/i18n/provider";

const methods = [
  { key: "password", label: "login.password", detail: "signup.passwordDetail", icon: "lock" },
  { key: "magic", label: "login.magic", detail: "signup.magicDetail", icon: "mail" },
] as const;

export function SignupView(props: SignupViewProps) {
  const { t } = useLocale();
  return (
    <AuthPage homeHref={props.homeHref}>
      <AuthMethods
        title={
          <>
            {t("signup.space")}
            <br className="max-[760px]:hidden" />{" "}
            <span className="text-primary">{t("signup.begin")}</span>
          </>
        }
        description={t("signup.choose")}
        label={t("signup.method")}
        options={methods.map((method) => ({
          ...method,
          label: t(method.label),
          detail: t(method.detail),
        }))}
        columns="two"
        method={props.method}
        disabled={props.methodsDisabled}
        onMethodChange={props.onMethodChange}
      />
      <AuthPanel label={t("signup.submit")} illustration={<EntryPaths method={props.method} />}>
        {props.children}
        <div className="mt-8 grid gap-3 pt-6 text-notice leading-relaxed text-ink max-[760px]:mt-[22px] max-[760px]:pt-4.5">
          <p>
            {t("signup.haveAccount")}{" "}
            <Link
              to="/login"
              className="inline-flex min-h-11 items-center font-medium text-primary hover:underline"
            >
              {t("signup.signIn")}
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
