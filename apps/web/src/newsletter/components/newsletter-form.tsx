import { Alert, Button, TextField, Label, FieldError, Input } from "@swarmie/ui-web";
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
    <Alert status="success" role={announce ? "status" : "presentation"}>
      <Alert.Description>{t("newsletter.complete")}</Alert.Description>
    </Alert>
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
        <TextField isInvalid={error !== undefined}>
          <Label htmlFor={`${id}-email`}>{t("newsletter.email")}</Label>
          <div className="flex flex-col gap-3 @lg:flex-row @lg:items-stretch">
            <div className="min-w-0 flex-1">
              <Input
                id={`${id}-email`}
                name="email"
                type="email"
                autoComplete="email"
                placeholder={t("newsletter.placeholder")}
                required
                maxLength={254}
                readOnly={pending}
              />
            </div>
            <Button
              type="submit"
              loading={pending}
              loadingLabel={t("newsletter.saving")}
              className="w-full text-center whitespace-normal @lg:w-auto @lg:max-w-[48%]"
            >
              {t("newsletter.submit")}
            </Button>
          </div>
          <FieldError>
            {error ? (
              <span role={announce === true ? "alert" : "presentation"}>{t(error)}</span>
            ) : undefined}
          </FieldError>
        </TextField>
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
