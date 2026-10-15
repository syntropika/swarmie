import { Button, SwarmieMark } from "@swarmie/ui-web";
import type { ReactElement } from "react";

import { Feedback } from "~/src/identity/shared/components/feedback";
import { LoginIcon } from "~/src/identity/shared/components/icon";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import type { PlainMessageKey } from "~/src/platform/i18n/catalogs/en";
import { useLocale } from "~/src/platform/i18n/provider";
import { MessagePage } from "~/src/platform/shell/components/message-page";

export type MagicLinkStage =
  | "loading"
  | "ready"
  | "consuming"
  | "unavailable"
  | "verify-email"
  | "additional-verification";

export type MagicLinkViewProps = {
  stage: MagicLinkStage;
  pending: boolean;
  homeHref: string;
  secureSignInHref: string;
  feedback?: LoginFeedback | undefined;
  verificationForm?: ReactElement | undefined;
  onContinue: () => void;
  onBackToSignIn: () => void;
};

const content: Record<MagicLinkStage, { title: PlainMessageKey; description?: PlainMessageKey }> = {
  loading: { title: "magic.opening" },
  ready: { title: "login.way", description: "magic.ready" },
  consuming: { title: "login.way", description: "magic.ready" },
  unavailable: { title: "magic.unavailableTitle", description: "magic.unavailable" },
  "verify-email": { title: "magic.oneMore" },
  "additional-verification": { title: "secure.title", description: "secure.description" },
};

function ContinueAction(
  props: Pick<MagicLinkViewProps, "stage" | "pending" | "secureSignInHref" | "onContinue">,
) {
  const { t } = useLocale();
  // oxlint-disable-next-line default-case -- Explicit stages remain checked for exhaustiveness.
  switch (props.stage) {
    case "ready":
    case "consuming": {
      return (
        <Button
          size="xl"
          className="w-full [overflow-wrap:anywhere] whitespace-normal"
          loading={props.pending}
          loadingLabel={t("common.signingIn")}
          disabled={props.stage !== "ready"}
          onClick={props.onContinue}
        >
          {t("common.continue")} <LoginIcon name="arrow" />
        </Button>
      );
    }
    case "additional-verification": {
      return (
        <Button size="xl" className="w-full [overflow-wrap:anywhere] whitespace-normal" asChild>
          <a href={props.secureSignInHref}>
            {t("secure.continue")} <LoginIcon name="arrow" />
          </a>
        </Button>
      );
    }
    case "loading":
    case "unavailable":
    case "verify-email": {
      return null;
    }
  }
  throw new Error("Unexpected magic link stage");
}

function MagicLinkActions(props: MagicLinkViewProps) {
  const { t } = useLocale();
  if (props.stage === "loading") {
    return (
      <p
        className="max-w-[65ch] text-text-muted"
        // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- This live region announces asynchronous status, not the result of a form calculation.
        role="status"
      >
        {t("magic.preparing")}
      </p>
    );
  }
  if (props.stage === "verify-email") {
    return props.verificationForm ?? null;
  }
  return (
    <div className="grid w-full gap-3">
      <ContinueAction {...props} />
      <Button
        variant="secondary"
        size="xl"
        className="w-full [overflow-wrap:anywhere] whitespace-normal"
        disabled={props.pending}
        onClick={props.onBackToSignIn}
      >
        {t(props.stage === "unavailable" ? "magic.requestNew" : "common.backSignIn")}
      </Button>
    </div>
  );
}

/** Present a deliberate sign-in action; opening this view never consumes an email link. */
export function MagicLinkView(props: MagicLinkViewProps) {
  const { t } = useLocale();
  const { title, description } = content[props.stage];
  return (
    <MessagePage>
      <a
        className="inline-flex min-h-control w-max items-center gap-2 rounded-pill text-brand leading-none font-semibold tracking-brand text-ink no-underline sm:text-brand-large pointer-coarse:min-h-control-touch"
        href={props.homeHref}
        aria-label={t("common.home")}
      >
        <SwarmieMark className="size-8" />
        swarmie
      </a>
      <div className="grid size-15 place-items-center rounded-panel bg-primary-soft text-primary">
        <LoginIcon name="mail" className="size-8" />
      </div>
      <h1 className="text-title font-medium text-balance">{t(title)}</h1>
      {description ? <p className="max-w-[65ch] text-text-muted">{t(description)}</p> : null}
      <MagicLinkActions {...props} />
      <Feedback feedback={props.feedback} />
    </MessagePage>
  );
}
