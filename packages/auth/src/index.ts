export { AuthenticationProvider } from "~/src/host";
export type { AuthenticationHost, AuthDestination, AuthNavigation } from "~/src/host";
export { createAuthEffects, runAuthEffect } from "~/src/identity/authentication/effects";
export type { AuthEffects } from "~/src/identity/authentication/effects";
export { LoginController } from "~/src/identity/login";
export { SignupController } from "~/src/identity/signup";
export { ResetPassword } from "~/src/identity/password-reset";
export { MagicLinkController } from "~/src/identity/magic-link";
export { PasskeyAccount } from "~/src/identity/passkeys";
export {
  createSessionQueryOptions,
  retainSessionQuery,
  observeSessionAccount,
  useSessionExpiry,
  useSessionObserver,
  useSignOut,
  AccountMenu,
  AccountView,
  SettingsSidebar,
  settingsSections,
  readCachedSession,
} from "~/src/identity/session";
export type { SettingsSection } from "~/src/identity/session";
export { clearAccountQueries, registerAccountResource } from "~/src/platform/account-cache";
export { LocaleProvider } from "~/src/platform/i18n/provider";
