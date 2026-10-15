import { NodeClient } from "~/web/src/nodes";
import type { NodesState } from "~/web/src/nodes";

export class DemoClient extends NodeClient {
  private readonly ownerReady = Promise.resolve();
  constructor(private readonly demoNodes: NodesState) {
    super(demoNodes.store);
  }
  override getSnapshot = () => this.demoNodes;
  override async select(environmentId: string) {
    if (!this.demoNodes.nodes.some((node) => node.identity.environmentId === environmentId)) {
      throw new Error("Unknown demo node");
    }
    await this.ownerReady;
  }
}
