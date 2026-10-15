import type { User } from "@swarmie/api";
import {
  Button,
  SelectContent,
  SelectItem,
  SelectRoot,
  SelectTrigger,
  SelectValue,
  Surface,
  SurfaceRow,
} from "@swarmie/ui-web";
import { useId } from "react";
import type { ReactNode } from "react";

import { Link } from "~/src/host";
import { settingsSectionLabels } from "~/src/identity/session/components/settings-sidebar";
import type { SettingsSection } from "~/src/identity/session/navigation";
import { locales } from "~/src/platform/i18n/locale";
import { isLocalePreference } from "~/src/platform/i18n/preference";
import { useLocale } from "~/src/platform/i18n/provider";
import { ThemeControl } from "~/src/platform/theme";

export type AccountViewProps = {
  user: User | undefined;
  section: SettingsSection;
  pending: boolean;
  error: boolean;
  invitationPending: boolean;
  onLogout: () => void;
  onRetry: () => void;
  children?: ReactNode;
};

function GeneralSettings() {
  const { t, locale, preference, setPreference } = useLocale();
  const id = useId();
  const names = new Intl.DisplayNames([locale], { type: "language" });
  return (
    <Surface inset="none">
      <SurfaceRow>
        <div>
          <label htmlFor={id} className="text-ui font-medium">
            {t("settings.language")}
          </label>
          <p id={`${id}-help`} className="mt-1 text-ui text-text-muted">
            {t("settings.languageHelp")}
          </p>
        </div>
        <div className="max-w-52 min-w-32 flex-1">
          <SelectRoot
            value={preference}
            onValueChange={(value) => {
              if (isLocalePreference(value)) {
                setPreference(value);
              }
            }}
          >
            <SelectTrigger
              id={id}
              aria-describedby={`${id}-help`}
              controlSize="compact"
              className="w-full"
            >
              <SelectValue>
                {preference === "automatic"
                  ? t("settings.languageAutomatic")
                  : (names.of(preference) ?? preference)}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="automatic">{t("settings.languageAutomatic")}</SelectItem>
              {locales.map((value) => (
                <SelectItem key={value} value={value}>
                  {names.of(value) ?? value}
                </SelectItem>
              ))}
            </SelectContent>
          </SelectRoot>
        </div>
      </SurfaceRow>
    </Surface>
  );
}

function AccountIdentity(props: AccountViewProps & { user: User }) {
  const { t } = useLocale();
  return (
    <div className="space-y-6">
      <Surface inset="none">
        <dl className="grid">
          <SurfaceRow>
            <dt className="text-ui font-medium">{t("settings.name")}</dt>
            <dd className="min-w-0 text-ui [overflow-wrap:anywhere]">
              {props.user.display_name ?? props.user.email}
            </dd>
          </SurfaceRow>
          <SurfaceRow>
            <dt className="text-ui font-medium">{t("settings.email")}</dt>
            <dd className="min-w-0 text-ui [overflow-wrap:anywhere]">{props.user.email}</dd>
          </SurfaceRow>
        </dl>
      </Surface>
      <Button
        variant="secondary"
        loading={props.pending}
        loadingLabel={t("account.signingOut")}
        onClick={props.onLogout}
      >
        {t("account.signOut")}
      </Button>
      {props.invitationPending ? (
        <div>
          <Link
            className="inline-flex min-h-control items-center text-primary underline underline-offset-4 pointer-coarse:min-h-control-touch"
            to="/invitations/accept"
          >
            {t("account.invitation")}
          </Link>
        </div>
      ) : null}
    </div>
  );
}

/** Section presentation keeps account requests, navigation, and credential workflows outside the view. */
export function AccountView(props: AccountViewProps) {
  const { t } = useLocale();
  function content(): ReactNode {
    switch (props.section) {
      case "general": {
        return <GeneralSettings />;
      }
      case "appearance": {
        return (
          <Surface inset="none">
            <SurfaceRow>
              <ThemeControl layout="row" />
            </SurfaceRow>
          </Surface>
        );
      }
      case "security": {
        return props.children;
      }
      case "account": {
        return props.user ? <AccountIdentity {...props} user={props.user} /> : null;
      }
      default: {
        const unhandled: never = props.section;
        return unhandled;
      }
    }
  }
  return (
    <main className="min-h-0 min-w-0 flex-1 overflow-y-auto px-5 pt-20 pb-12 sm:px-8 md:pt-8 lg:px-12">
      <div className="mx-auto w-full max-w-3xl">
        <h1 className="mb-5 text-title font-medium">{t(settingsSectionLabels[props.section])}</h1>
        {content()}
        {props.error ? (
          <div className="mt-6 space-y-2">
            <p role="alert" className="text-ui text-danger">
              {t("account.connection")}
            </p>
            <Button variant="link" onClick={props.onRetry}>
              {t("common.retry")}
            </Button>
          </div>
        ) : null}
      </div>
    </main>
  );
}
