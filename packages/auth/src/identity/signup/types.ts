import type { ReactNode } from "react";

import type { LoginFeedback } from "~/src/identity/shared/feedback";
import type { SignupMethod } from "~/src/identity/signup/model";

/** Controlled inputs keep signup presentation independent of authentication and navigation. */
export type SignupFieldsProps = {
  method: SignupMethod;
  email: string;
  password: string;
  showPassword: boolean;
  pending: boolean;
  feedback: LoginFeedback | undefined;
  onEmailChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onTogglePassword: () => void;
  onSubmit: () => void;
};

export type SignupViewProps = {
  homeHref: string;
  method: SignupMethod;
  methodsDisabled: boolean;
  onMethodChange: (method: SignupMethod) => void;
  children: ReactNode;
};
