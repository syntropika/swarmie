import { Button, Input } from "@swarmie/ui-web";
import { useId } from "react";

import { Feedback } from "~/src/identity/shared/components/feedback";
import { LoginIcon } from "~/src/identity/shared/components/icon";
import type { LoginFeedback } from "~/src/identity/shared/feedback";
import { useLocale } from "~/src/platform/i18n/provider";

export type PasskeyListItem = {
  id: string;
  name: string;
  createdAt: string;
  lastUsedAt: string | null;
};

export type PasskeyManagementProps = {
  passkeys: PasskeyListItem[];
  name: string;
  loading: boolean;
  pending: "register" | { revokeId: string } | null;
  available: boolean;
  feedback?: LoginFeedback | undefined;
  onNameChange: (name: string) => void;
  onRegister: () => void;
  onRevoke: (id: string) => void;
  onCancel?: () => void;
};

type PasskeyListProps = Pick<
  PasskeyManagementProps,
  "passkeys" | "loading" | "pending" | "onRevoke"
>;

function PasskeyItem({
  passkey,
  pending,
  onRevoke,
}: { passkey: PasskeyListItem } & Pick<PasskeyListProps, "pending" | "onRevoke">) {
  const { t, formatDate } = useLocale();
  const removing =
    pending !== null && typeof pending === "object" && pending.revokeId === passkey.id;
  const lastUsed =
    passkey.lastUsedAt === null
      ? t("keys.unused")
      : t("keys.usedAt", { date: formatDate(passkey.lastUsedAt) });
  return (
    <li className="flex items-start gap-3 py-2">
      <span className="mt-0.75 shrink-0 text-primary">
        <LoginIcon name="key" />
      </span>
      <div className="min-w-0 flex-1">
        <h3 className="mb-[5px] text-ui font-medium [overflow-wrap:anywhere]">{passkey.name}</h3>
        <p className="text-xs text-text-muted">
          {t("keys.addedAt", { date: formatDate(passkey.createdAt) })}
        </p>
        <p className="text-xs text-text-muted">{lastUsed}</p>
      </div>
      <Button
        variant="ghost"
        size="sm"
        disabled={pending !== null}
        loading={removing}
        loadingLabel={t("keys.removing", { name: passkey.name })}
        aria-label={t("keys.removeNamed", { name: passkey.name })}
        onClick={() => {
          onRevoke(passkey.id);
        }}
      >
        {t("keys.remove")}
      </Button>
    </li>
  );
}

function PasskeyList(props: PasskeyListProps) {
  const { t } = useLocale();
  if (props.loading) {
    return (
      <p
        className="max-w-[65ch] text-text-muted"
        // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- This live region announces asynchronous status, not the result of a form calculation.
        role="status"
      >
        {t("keys.loading")}
      </p>
    );
  }
  if (props.passkeys.length === 0) {
    return <p className="text-ui text-text-muted">{t("keys.empty")}</p>;
  }
  return (
    <ul className="m-0 list-none p-0" aria-label={t("keys.list")}>
      {props.passkeys.map((passkey) => (
        <PasskeyItem
          key={passkey.id}
          passkey={passkey}
          pending={props.pending}
          onRevoke={props.onRevoke}
        />
      ))}
    </ul>
  );
}

type RegistrationProps = Pick<
  PasskeyManagementProps,
  "name" | "loading" | "pending" | "available" | "onNameChange" | "onRegister" | "onCancel"
>;

function PasskeyRegistrationForm(props: RegistrationProps) {
  const id = useId();
  const { t } = useLocale();
  const busy = props.pending !== null;
  const registering = props.pending === "register";
  const disabled = busy || !props.available || props.loading;
  return (
    <form
      className="grid w-full gap-5.5"
      aria-busy={registering}
      onSubmit={(event) => {
        event.preventDefault();
        props.onRegister();
      }}
    >
      <div className="grid gap-2">
        <label className="text-ui font-medium" htmlFor={id}>
          {t("keys.name")}
        </label>
        <Input
          controlSize="large"
          id={id}
          name="passkey-name"
          placeholder={t("keys.namePlaceholder")}
          value={props.name}
          required
          maxLength={80}
          disabled={disabled}
          onChange={(event) => {
            props.onNameChange(event.target.value);
          }}
          aria-describedby={`${id}-help`}
        />
        <p id={`${id}-help`} className="text-xs text-text-muted">
          {t("keys.nameHelp")}
        </p>
      </div>
      <Button
        size="xl"
        className="mt-0.5 w-full [overflow-wrap:anywhere] whitespace-normal"
        type="submit"
        disabled={disabled}
        loading={registering}
        loadingLabel={t("keys.waiting")}
      >
        <LoginIcon name="key" />
        {t("keys.add")}
      </Button>
      {registering && props.onCancel ? (
        <Button variant="link" className="justify-start whitespace-normal" onClick={props.onCancel}>
          {t("common.cancel")}
        </Button>
      ) : null}
      {props.available ? null : (
        <p className="text-body leading-normal text-text-muted max-[760px]:text-notice">
          {t("keys.unavailable")}
        </p>
      )}
    </form>
  );
}

/** Present saved passkeys and lifecycle actions; credentials and API calls stay with the controller. */
export function PasskeyManagement(props: PasskeyManagementProps) {
  const id = useId();
  const { t } = useLocale();
  return (
    <section className="mt-3 grid w-full gap-6 pt-4" aria-labelledby={id}>
      <div className="grid gap-2">
        <h2 className="text-title font-medium text-balance" id={id}>
          {t("keys.title")}
        </h2>
        <p className="text-ui text-text-muted">{t("keys.description")}</p>
      </div>
      <PasskeyList
        passkeys={props.passkeys}
        loading={props.loading}
        pending={props.pending}
        onRevoke={props.onRevoke}
      />
      <PasskeyRegistrationForm
        name={props.name}
        loading={props.loading}
        pending={props.pending}
        available={props.available}
        onNameChange={props.onNameChange}
        onRegister={props.onRegister}
        {...(props.onCancel ? { onCancel: props.onCancel } : {})}
      />
      <Feedback feedback={props.feedback} />
    </section>
  );
}
