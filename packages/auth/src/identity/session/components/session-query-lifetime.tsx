import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { useSessionQueryOptions } from "~/src/host";
import { retainSessionQuery } from "~/src/identity/session/retain-query";

/** A browser application keeps its admitted session request through internal view transitions. */
export function SessionQueryLifetime() {
  const sessionQueryOptions = useSessionQueryOptions();
  const client = useQueryClient();
  useEffect(() => retainSessionQuery(client, sessionQueryOptions), [client, sessionQueryOptions]);
  return null;
}
