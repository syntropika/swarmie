import { useLocale } from "~/src/platform/i18n/provider";

/** A quiet, centered sign-off for focused account and workspace setup pages. */
export function PageFooter() {
  const { t } = useLocale();
  return (
    <footer className="mt-auto flex justify-center px-4 py-6 text-center text-xs leading-relaxed text-text-muted">
      <p className="max-w-md">{t("login.footer")}</p>
    </footer>
  );
}
