import type { ReactNode } from "react";

import { AccountContext } from "~/web/src/account-state";
import type { AccountStore } from "~/web/src/account-state";

export function AccountProvider({ store, children }: { store: AccountStore; children: ReactNode }) {
  return <AccountContext value={store}>{children}</AccountContext>;
}
