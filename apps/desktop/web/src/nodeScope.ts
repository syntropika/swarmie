import type { LocalStoreIdentity } from "@swarmie/contracts";

export function validNodeIdentity(value: unknown): value is LocalStoreIdentity {
  const node = value as LocalStoreIdentity | undefined;
  return !!node && [node.environmentId, node.storeId, node.ownerScopeId].every(
    (id) => typeof id === "string" && !!id && id.length <= 128 && !/[\u0000-\u001f\u007f]/.test(id),
  );
}

export function sameNode(a: LocalStoreIdentity, b: LocalStoreIdentity): boolean {
  return nodeKey(a) === nodeKey(b);
}

export function nodeKey(node: LocalStoreIdentity): string {
  if (!validNodeIdentity(node)) throw new Error("Invalid retained node identity.");
  return JSON.stringify([node.environmentId, node.storeId, node.ownerScopeId]);
}

export function retainNode(node: LocalStoreIdentity): LocalStoreIdentity {
  nodeKey(node);
  return Object.freeze({
    environmentId: node.environmentId,
    storeId: node.storeId,
    ownerScopeId: node.ownerScopeId,
  });
}

export function workspaceStorageKey(node: LocalStoreIdentity): string {
  return `swarmie.workspace.v3:${nodeKey(node)}`;
}

// Transport paths are same-origin routes. Additional node routing is supplied by the client host.
export function rpcPath(path = "/rpc"): string {
  if (!path.startsWith("/") || path.startsWith("//") || /[?#\\\s]/.test(path))
    throw new Error("A node transport requires a same-origin RPC path.");
  return path;
}

export function routeTargetKey(path: string): string {
  return `swarmie.runtime.target.v2:${rpcPath(path)}`;
}
