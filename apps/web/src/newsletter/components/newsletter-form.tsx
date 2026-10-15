import { AlertRoot, AlertDescription, Button, Field, Input } from "@swarmie/ui-web";
import { useId } from "react";

import type { PlainMessageKey } from "~/src/platform/i18n/catalogs/en";
import { useLocale } from "~/src/platform/i18n/provider";

type NewsletterFormProps = {
  pending: boolean;
  complete: boolean;
  error: PlainMessageKey | undefined;
  subscribe: (email: string, website: string) => void;
  announce?: boolean;
};

function SubscriptionConfirmation({ announce }: { announce: boolean }) {
  const { t } = useLocale();
  return (
    <AlertRoot
      tone="success"
      appearance="soft"
      spacing="compact"
      role={announce ? "status" : "presentation"}
    >
      <AlertDescription size="body" className="mt-0">
        {t("newsletter.complete")}
      </AlertDescription>
    </AlertRoot>
  );
}

function SubscriptionFields({
  pending,
  error,
  subscribe,
  announce,
}: Omit<NewsletterFormProps, "complete">) {
  const { t } = useLocale();
  const id = useId();
  return (
    <form
      action="/api/subscribe"
      method="post"
      aria-busy={pending}
      onSubmit={(event) => {
        event.preventDefault();
        const fields = new FormData(event.currentTarget);
        const email = fields.get("email");
        const website = fields.get("website");
        subscribe(
          typeof email === "string" ? email : "",
          typeof website === "string" ? website : "",
        );
      }}
    >
      <div className="@container">
        <Field
          id={`${id}-email`}
          label={t("newsletter.email")}
          error={
            error ? (
              <span role={announce === true ? "alert" : "presentation"}>{t(error)}</span>
            ) : undefined
          }
        >
          {(control) => (
            <div className="flex flex-col gap-3 @lg:flex-row @lg:items-stretch">
              <div className="min-w-0 flex-1">
                <Input
                  {...control}
                  controlSize="large"
                  name="email"
                  type="email"
                  autoComplete="email"
                  placeholder={t("newsletter.placeholder")}
                  required
                  maxLength={254}
                  readOnly={pending}
                  className="h-full"
                />
              </div>
              <Button
                type="submit"
                size="xl"
                loading={pending}
                loadingLabel={t("newsletter.saving")}
                className="w-full text-center whitespace-normal @lg:w-auto @lg:max-w-[48%]"
              >
                {t("newsletter.submit")}
              </Button>
            </div>
          )}
        </Field>
      </div>
      <div hidden aria-hidden="true">
        <label htmlFor={`${id}-website`}>{t("newsletter.website")}</label>
        <input id={`${id}-website`} name="website" type="text" tabIndex={-1} autoComplete="off" />
      </div>
      <p className="mt-3 max-w-[60ch] text-xs leading-6 text-pretty text-text-muted">
        {t("newsletter.privacy")}
      </p>
    </form>
  );
}

/** Controlled rendering; subscription transport and lifecycle belong to the newsletter hook. */
export function NewsletterForm(props: NewsletterFormProps) {
  return (
    <div className="mt-8 w-full max-w-[560px]">
      {props.complete ? (
        <SubscriptionConfirmation announce={props.announce === true} />
      ) : (
        <SubscriptionFields {...props} />
      )}
    </div>
  );
}
