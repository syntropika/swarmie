import { useAuthentication } from "~/src/host";
import { MagicLinkView } from "~/src/identity/magic-link/components/magic-link-view";
import { useMagicLink } from "~/src/identity/magic-link/hooks/use-magic-link";
import { ChallengeForm } from "~/src/identity/shared/components/challenge-form";

export function MagicLinkController() {
  const { client: api, siteUrl } = useAuthentication();

  const flow = useMagicLink();
  return (
    <MagicLinkView
      stage={flow.stage}
      pending={flow.pending}
      homeHref={siteUrl}
      secureSignInHref={api.loginUrl}
      feedback={flow.feedback}
      onContinue={flow.submit}
      onBackToSignIn={flow.backToSignIn}
      verificationForm={
        flow.stage === "verify-email" ? (
          <ChallengeForm
            kind="verify-email"
            value={flow.code}
            pending={flow.pending}
            feedback={undefined}
            onChange={flow.setCode}
            onSubmit={flow.submit}
            onBack={flow.backToSignIn}
          />
        ) : undefined
      }
    />
  );
}
