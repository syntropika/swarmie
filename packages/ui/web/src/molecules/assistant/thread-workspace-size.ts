/** Persisted presentation geometry, independent of a conversation or provider. */
export type WorkspaceSize = { width: number; height: number };
export const DEFAULT_WORKSPACE_SIZE: WorkspaceSize = { width: 400, height: 500 };

/** Small viewports take precedence over nominal desktop minimums. */
export function clampWorkspaceSize(size: WorkspaceSize, viewport: WorkspaceSize): WorkspaceSize {
  const maximumWidth = Math.max(1, viewport.width - 32);
  const maximumHeight = Math.max(1, viewport.height - 96);
  return {
    width: Math.round(Math.min(maximumWidth, Math.max(Math.min(320, maximumWidth), size.width))),
    height: Math.round(
      Math.min(maximumHeight, Math.max(Math.min(400, maximumHeight), size.height)),
    ),
  };
}

/** Untrusted storage never supplies NaN, Infinity or arbitrary object properties to geometry. */
export function parseWorkspaceSize(raw: string | null): WorkspaceSize | undefined {
  if (raw === null) {
    return undefined;
  }
  try {
    const value: unknown = JSON.parse(raw);
    if (
      typeof value !== "object" ||
      value === null ||
      !("width" in value) ||
      !("height" in value)
    ) {
      return undefined;
    }
    if (
      typeof value.width !== "number" ||
      typeof value.height !== "number" ||
      !Number.isFinite(value.width) ||
      !Number.isFinite(value.height)
    ) {
      return undefined;
    }
    return { width: value.width, height: value.height };
  } catch {
    return undefined;
  }
}
