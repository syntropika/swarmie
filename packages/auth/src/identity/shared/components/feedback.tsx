import { AlertRoot, AlertIcon, AlertContent, AlertTitle, AlertDescription } from "@swarmie/ui-web";

import { LoginIcon } from "~/src/identity/shared/components/icon";
import type { LoginIconName } from "~/src/identity/shared/components/icon";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import { useLocale } from "~/src/platform/i18n/provider";

const icons: Record<LoginFeedback["tone"], LoginIconName> = {
  info: "mail",
  error: "attention",
  success: "check",
};

/** Translation and authentication state stay here; the UI library supplies reusable notice parts. */
export function Feedback({ feedback }: { feedback: LoginFeedback | undefined }) {
  const { t } = useLocale();
  if (!feedback) {
    return null;
  }
  return (
    <AlertRoot tone={feedback.tone}>
      <AlertIcon>
        <LoginIcon name={icons[feedback.tone]} />
      </AlertIcon>
      <AlertContent>
        <AlertTitle>{t(feedback.title)}</AlertTitle>
        <AlertDescription>{t(feedback.message)}</AlertDescription>
      </AlertContent>
    </AlertRoot>
  );
}
