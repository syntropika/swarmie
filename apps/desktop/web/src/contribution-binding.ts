import type { ContributionContext, ScopedClientApi, ContributionRegistry } from "./contributions";

/** API scopes start on subscription and are always disposed before a view leaves its lifetime. */
export class ContributionBinding {
  private api?: ScopedClientApi;
  constructor(
    private readonly registry: ContributionRegistry,
    private readonly owner: string,
    private readonly context?: ContributionContext,
  ) {}
  getSnapshot = () => this.api;
  subscribe = (changed: () => void) => {
    const api = this.registry.api(this.owner, this.context);
    this.api = api;
    const aborted = () => {
      this.api = undefined;
      changed();
    };
    api.signal.addEventListener("abort", aborted, { once: true });
    changed();
    return () => {
      api.signal.removeEventListener("abort", aborted);
      api.dispose();
      this.api = undefined;
    };
  };
}
