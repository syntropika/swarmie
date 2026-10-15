import {
  ActionPopoverContent,
  ActionPopoverItem,
  ActionPopoverRoot,
  ActionPopoverTrigger,
  Avatar,
  Icon,
  Separator,
  useActionPopover,
} from "@swarmie/ui-web";
import { useEffect } from "react";

import { useLocale } from "~/src/platform/i18n/provider";

type AccountMenuProps = {
  displayName: string;
  email: string;
  initial: string;
  pending: boolean;
  error: boolean;
  onSettings: () => void;
  onSignOut: () => void;
};

function AccountMenuContents(props: AccountMenuProps) {
  const { t } = useLocale();
  const { close } = useActionPopover();
  useEffect(() => {
    const desktop = window.matchMedia("(min-width: 768px)");
    desktop.addEventListener("change", close);
    return () => {
      desktop.removeEventListener("change", close);
    };
  }, [close]);

  return (
    <ActionPopoverContent
      aria-label={t("account.menu")}
      className="inset-auto bottom-3 left-[52px]"
    >
      <div className="flex items-center gap-3 px-3 py-2.5">
        <Avatar kind="person" size="sm" initials={props.initial} label={props.displayName} />
        <div className="min-w-0">
          <p className="font-medium wrap-break-word">{props.displayName}</p>
          {props.displayName === props.email ? null : (
            <p className="text-xs break-all text-text-muted">{props.email}</p>
          )}
        </div>
      </div>
      <div className="px-3 py-1.5">
        <Separator />
      </div>
      <ActionPopoverItem
        disabled={props.pending}
        onClick={() => {
          close();
          props.onSettings();
        }}
      >
        <Icon name="settings" />
        {t("settings.title")}
      </ActionPopoverItem>
      <div className="px-3 py-1.5">
        <Separator />
      </div>
      <ActionPopoverItem
        disabled={props.pending}
        aria-busy={props.pending ? true : undefined}
        onClick={props.onSignOut}
      >
        <Icon name="logout" />
        {t(props.pending ? "account.signingOut" : "account.signOut")}
      </ActionPopoverItem>
      {props.error ? (
        <p role="alert" className="px-3 py-2 text-ui text-danger">
          {t("account.signOutFailed")}
        </p>
      ) : null}
    </ActionPopoverContent>
  );
}

/** Account navigation and logout state belong to the caller; this menu only presents actions. */
export function AccountMenu(props: AccountMenuProps) {
  const { t } = useLocale();
  return (
    <ActionPopoverRoot>
      <ActionPopoverTrigger aria-label={t("account.menu")} title={props.displayName}>
        <Avatar kind="person" size="sm" initials={props.initial} label={props.displayName} />
      </ActionPopoverTrigger>
      <AccountMenuContents {...props} />
    </ActionPopoverRoot>
  );
}
