import { Button, cn } from "@swarmie/ui-web";
import type { ReactNode } from "react";

import type { AuthenticationMethod } from "~/src/identity/shared/authentication-method";
import { LoginIcon } from "~/src/identity/shared/components/icon";
import type { LoginIconName } from "~/src/identity/shared/components/icon";

type AuthMethodOption<Method extends AuthenticationMethod> = {
  key: Method;
  label: string;
  detail: string;
  icon: LoginIconName;
};

type AuthMethodsProps<Method extends AuthenticationMethod> = {
  title: ReactNode;
  description: string;
  label: string;
  options: readonly AuthMethodOption<Method>[];
  columns: "two" | "three";
  method: Method;
  disabled: boolean;
  onMethodChange: (method: Method) => void;
};

const methodTones = { password: "mint", magic: "peach", passkey: "lilac" } as const;

/** Equally sized method controls share one responsive layout across identity entry points. */
export function AuthMethods<Method extends AuthenticationMethod>(props: AuthMethodsProps<Method>) {
  return (
    <section className="min-w-0 self-center">
      <h1 className="text-title font-medium text-balance">{props.title}</h1>
      <p className="mt-5 max-w-md text-body leading-relaxed text-text-muted">{props.description}</p>
      <fieldset
        className={cn(
          "m-0 mt-7 grid min-w-0 gap-2 border-0 p-0 @3xl:mt-9 @3xl:grid-cols-1 @3xl:gap-3",
          props.columns === "two" ? "grid-cols-2" : "grid-cols-3",
        )}
        aria-label={props.label}
        disabled={props.disabled}
      >
        {props.options.map((option) => (
          <Button
            size="auth"
            variant="choice"
            selected={props.method === option.key}
            selectionTone={methodTones[option.key]}
            key={option.key}
            aria-pressed={props.method === option.key}
            className="group w-full min-w-0 justify-start text-left whitespace-normal"
            onClick={() => {
              props.onMethodChange(option.key);
            }}
          >
            <LoginIcon name={option.icon} className="size-5 shrink-0" />
            <span className="min-w-0 flex-1">
              <strong className="block text-xs leading-4 font-medium [overflow-wrap:anywhere] @3xl:text-body @3xl:leading-body">
                {option.label}
              </strong>
              <small className="mt-1 hidden text-ui leading-ui text-text-muted group-aria-pressed:text-ink @3xl:block">
                {option.detail}
              </small>
            </span>
            <span className="hidden opacity-0 group-aria-pressed:opacity-100 @3xl:block">
              <LoginIcon name="arrow" />
            </span>
          </Button>
        ))}
      </fieldset>
    </section>
  );
}
