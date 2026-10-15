import { Button } from "@swarmie/ui-web";

import {
  EmailField,
  NewPasswordField,
  VerificationCodeField,
} from "~/src/identity/shared/components/auth-fields";
import { AuthForm, AuthSubmit } from "~/src/identity/shared/components/auth-form";
import { Feedback } from "~/src/identity/shared/components/feedback";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import { useLocale } from "~/src/platform/i18n/provider";

type ChallengeFormProps = {
  kind: "verify-email" | "request-reset" | "reset-password";
  value: string;
  pending: boolean;
  feedback: LoginFeedback | undefined;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onBack: () => void;
};

const challenges = {
  "verify-email": {
    title: "verify.title",
    description: "verify.description",
    submit: "verify.submit",
    Field: VerificationCodeField,
  },
  "request-reset": {
    title: "reset.title",
    description: "reset.description",
    submit: "reset.submit",
    Field: EmailField,
  },
  "reset-password": {
    title: "reset.newTitle",
    description: "reset.newDescription",
    submit: "reset.save",
    Field: NewPasswordField,
  },
} as const;

/** A controlled secondary form; token handling stays in the authentication hook. */
export function ChallengeForm(props: ChallengeFormProps) {
  const { t } = useLocale();
  const { title, description, submit, Field } = challenges[props.kind];
  return (
    <AuthForm pending={props.pending} onSubmit={props.onSubmit}>
      <h2 className="text-title font-medium text-balance">{t(title)}</h2>
      <p className="text-body leading-normal text-text-muted max-[760px]:text-notice">
        {t(description)}
      </p>
      <Field value={props.value} disabled={props.pending} onChange={props.onChange} />
      <AuthSubmit
        className="mt-0.5"
        label={t(submit)}
        pending={props.pending}
        loadingLabel={t("common.wait")}
        showArrow={false}
      />
      <Feedback feedback={props.feedback} />
      <Button
        variant="link"
        className="justify-start whitespace-normal"
        disabled={props.pending}
        onClick={props.onBack}
      >
        {t("common.backSignIn")}
      </Button>
    </AuthForm>
  );
}
