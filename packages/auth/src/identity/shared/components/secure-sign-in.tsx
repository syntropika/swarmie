import { Button } from "@swarmie/ui-web";

import { useLocale } from "~/src/platform/i18n/provider";

type SecureSignInProps = {
  href: string;
  onBack: () => void;
};

export function SecureSignIn({ href, onBack }: SecureSignInProps) {
  const { t } = useLocale();
  return (
    <div className="grid w-full gap-5.5">
      <h2 className="text-title font-medium text-balance">{t("secure.title")}</h2>
      <p className="text-body leading-normal text-text-muted max-[760px]:text-notice">
        {t("secure.description")}
      </p>
      <Button
        size="xl"
        className="mt-0.5 w-full [overflow-wrap:anywhere] whitespace-normal"
        asChild
      >
        <a href={href}>{t("secure.continue")}</a>
      </Button>
      <Button variant="link" className="justify-start whitespace-normal" onClick={onBack}>
        {t("common.backSignIn")}
      </Button>
    </div>
  );
}
