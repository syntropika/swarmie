export function reviewKey(
  worktreeId: string,
  scope: string,
  base: string,
  file: string,
): string {
  return JSON.stringify([worktreeId, scope, base, file]);
}
export async function fingerprint(content: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(content),
  );
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}
