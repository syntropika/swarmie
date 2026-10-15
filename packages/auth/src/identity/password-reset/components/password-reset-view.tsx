import * as Match from "effect/Match";

import { Link } from "~/src/host";
import { ChallengeForm } from "~/src/identity/shared/components/challenge-form";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import { useLocale } from "~/src/platform/i18n/provider";
import { MessagePage } from "~/src/platform/shell/components/message-page";

type PasswordResetViewProps = {
  stage: "incomplete" | "complete" | "form";
  password: string;
  pending: boolean;
  feedback: LoginFeedback | undefined;
  changePassword: (value: string) => void;
  submit: () => void;
  onBack: () => void;
};

export function PasswordResetView(props: PasswordResetViewProps) {
  const { t } = useLocale();
  return (
    <MessagePage>
      {Match.value(props.stage).pipe(
        Match.when("complete", () => (
          <>
            <h1 className="text-title font-medium text-balance">{t("reset.completeTitle")}</h1>
            <p className="max-w-[65ch] text-text-muted">{t("reset.complete")}</p>
            <Link className="text-primary underline underline-offset-4" to="/login">
              {t("common.backSignIn")}
            </Link>
          </>
        )),
        Match.when("incomplete", () => (
          <>
            <h1 className="text-title font-medium text-balance">{t("reset.incompleteTitle")}</h1>
            <p className="max-w-[65ch] text-text-muted">{t("reset.incomplete")}</p>
            <Link className="text-primary underline underline-offset-4" to="/login">
              {t("common.backSignIn")}
            </Link>
          </>
        )),
        Match.when("form", () => (
          <ChallengeForm
            kind="reset-password"
            value={props.password}
            pending={props.pending}
            feedback={props.feedback}
            onChange={props.changePassword}
            onSubmit={props.submit}
            onBack={props.onBack}
          />
        )),
        Match.exhaustive,
      )}
    </MessagePage>
  );
}
