import type { ReactNode } from "react";

import { AccountContext } from "~/src/account-state";
import type { AccountStore } from "~/src/account-state";

export function AccountProvider({ store, children }: { store: AccountStore; children: ReactNode }) {
  return <AccountContext value={store}>{children}</AccountContext>;
}
