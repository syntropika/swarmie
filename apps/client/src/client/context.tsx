import { createContext, useContext } from "react";

import type { ClientUi } from "./initialize";

export const ClientContext = createContext<ClientUi | null>(null);
export function useClient(): ClientUi {
  const client = useContext(ClientContext);
  if (client === null) {
    throw new Error("Client presentation requires an initialized frontend host.");
  }
  return client;
}
