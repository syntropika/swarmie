import { accountQueryKeys } from "~/src/platform/account-cache";

export const authQueryKeys = {
  all: [...accountQueryKeys.all, "auth"] as const,
  session: [...accountQueryKeys.all, "auth", "session"] as const,
  passkeys: (userId: string) => [...accountQueryKeys.all, "auth", "passkeys", userId] as const,
};
