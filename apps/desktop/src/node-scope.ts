import type { LocalStoreIdentity } from "@swarmie/contracts";

export function validNodeIdentity<T>(value: T): value is T & LocalStoreIdentity {
  if (
    typeof value !== "object" ||
    value === null ||
    !("environmentId" in value) ||
    !("storeId" in value) ||
    !("ownerScopeId" in value)
  ) {
    return false;
  }
  return [value.environmentId, value.storeId, value.ownerScopeId].every(
    (id) => typeof id === "string" && id.length > 0 && id.length <= 128 && !hasControlCharacter(id),
  );
}

export function sameNode(a: LocalStoreIdentity, b: LocalStoreIdentity): boolean {
  return nodeKey(a) === nodeKey(b);
}

export function nodeKey(node: LocalStoreIdentity): string {
  if (!validNodeIdentity(node)) {
    throw new Error("Invalid retained node identity.");
  }
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
  if (!path.startsWith("/") || path.startsWith("//") || /[?#\\\s]/u.test(path)) {
    throw new Error("A node transport requires a same-origin RPC path.");
  }
  return path;
}

export function routeTargetKey(path: string): string {
  return `swarmie.runtime.target.v2:${rpcPath(path)}`;
}

function hasControlCharacter(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.codePointAt(index) ?? 0;
    if (code <= 31 || code === 127) {
      return true;
    }
  }
  return false;
}
