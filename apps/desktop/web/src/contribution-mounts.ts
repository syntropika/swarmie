import type { ReactNode } from "react";

import type { ScopedClientApi } from "~/web/src/contributions";

export type Mount = (props: { api: ScopedClientApi }) => ReactNode;
/** Mount registrations are host-owned; callers receive only the stable lookup interface. */
export class ContributionMounts {
  private readonly entries = new Map<string, Mount>();
  constructor(id: string, mount: Mount) {
    this.entries.set(id, mount);
  }
  set(id: string, mount: Mount) {
    this.entries.set(id, mount);
  }
  delete(id: string) {
    return this.entries.delete(id);
  }
  get(id: string) {
    return this.entries.get(id);
  }
  has(id: string) {
    return this.entries.has(id);
  }
}
