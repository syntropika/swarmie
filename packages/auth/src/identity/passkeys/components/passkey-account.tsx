import { PasskeyManagement } from "~/src/identity/passkeys/components/passkey-management";
import { usePasskeyAccount } from "~/src/identity/passkeys/hooks/use-passkey-account";

/** Compose the credential lifecycle hook with its fully controlled management view. */
export function PasskeyAccount({ userId }: { userId: string }) {
  return <PasskeyManagement {...usePasskeyAccount(userId)} />;
}
