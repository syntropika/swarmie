import type { ReactNode } from "react";

import { AccountContext } from "./account-state";
import type { AccountStore } from "./account-state";

export function AccountProvider({ store, children }: { store: AccountStore; children: ReactNode }) {
  return <AccountContext value={store}>{children}</AccountContext>;
}
