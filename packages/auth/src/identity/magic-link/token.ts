/** Copy the opaque token into memory and remove every fragment value from the current history entry. */
export function takeMagicLinkToken(
  location: Pick<Location, "hash" | "pathname" | "search">,
  history: Pick<History, "state" | "replaceState">,
): string | null {
  const fragment = location.hash;
  if (fragment) {
    history.replaceState(history.state, "", `${location.pathname}${location.search}`);
  }
  const tokens = new URLSearchParams(fragment.replace(/^#/u, "")).getAll("token");
  const [token] = tokens;
  return tokens.length === 1 && token !== undefined && /^[A-Za-z0-9_-]{43}$/u.test(token)
    ? token
    : null;
}
