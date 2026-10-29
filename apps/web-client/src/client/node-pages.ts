import { Code, TembraError } from "@syntropika/tembra-sdk";

/** Cursor-dependent requests are sequential. Each node response has a bounded frame size. */
export async function readNodePages<Page extends { nextCursor: string | null }>(
  read: (cursor: string | null) => Promise<Page>,
  revision?: { reason: string; current: () => boolean },
): Promise<Page[]> {
  async function collect() {
    const pages: Page[] = [];
    const seen = new Set<string>();
    async function visit(cursor: string | null): Promise<Page[]> {
      const page = await read(cursor);
      pages.push(page);
      if (page.nextCursor === null) {
        return pages;
      }
      if (seen.has(page.nextCursor) || pages.length >= 4096) {
        throw new Error("The node catalog did not finish loading.");
      }
      seen.add(page.nextCursor);
      return await visit(page.nextCursor);
    }
    return await visit(null);
  }
  try {
    return await collect();
  } catch (error: unknown) {
    if (
      revision !== undefined &&
      revision.current() &&
      error instanceof TembraError &&
      error.code === Code.Conflict &&
      error.reason === revision.reason
    ) {
      // Only the read restarts. Discard pages from the old catalog revision.
      return await collect();
    }
    throw error;
  }
}
