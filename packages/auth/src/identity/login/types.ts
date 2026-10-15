import type { ReactNode } from "react";

import type { AuthenticationMethod } from "~/src/identity/shared/authentication-method";
import type { LoginFeedback } from "~/src/identity/shared/feedback";

/** Sign-in choices supported by the Swarmie login interface. */
export type LoginMethod = AuthenticationMethod;

/** Controlled presentation state; authentication and navigation stay with the caller. */
export type LoginViewProps = {
  method: LoginMethod;
  email: string;
  password: string;
  showPassword: boolean;
  pending: boolean;
  passkeyAvailable?: boolean;
  feedback?: LoginFeedback | undefined;
  formContent?: ReactNode;
  introduction?: { title: string; description: string };
  homeHref: string;
  onMethodChange: (method: LoginMethod) => void;
  onEmailChange: (email: string) => void;
  onPasswordChange: (password: string) => void;
  onTogglePassword: () => void;
  onSubmit: () => void;
  onForgotPassword: () => void;
};
