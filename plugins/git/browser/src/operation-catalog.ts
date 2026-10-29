import type {
  ArchiveReceipt,
  GitReceipt,
  GitRequest,
  GitResponse,
  PageCursor,
  RepositoryTarget,
  UnavailableEntry,
} from "./protocol";

/** Read the complete durable catalog before allowing another effect in this checkout. */
export async function readOperations(
  remote: { request: (request: GitRequest) => Promise<GitResponse> },
  target: RepositoryTarget,
) {
  const operations: GitReceipt[] = [];
  const unavailable: UnavailableEntry[] = [];
  async function readPage(page: PageCursor | null): Promise<void> {
    const result = await remote.request({
      kind: "operations",
      target,
      ...(page === null ? {} : { page }),
    });
    if (result.kind !== "operations") {
      throw new Error("The Git node returned an unexpected operation catalog.");
    }
    operations.push(...result.operations);
    unavailable.push(...result.unavailable);
    if (
      result.next_page !== null &&
      (result.next_page.offset !== operations.length ||
        result.next_page.offset <= (page?.offset ?? 0) ||
        (page !== null && result.next_page.digest !== page.digest))
    ) {
      throw new Error(
        "The Git operation catalog changed during recovery. Refresh before continuing.",
      );
    }
    if (result.next_page !== null) {
      await readPage(result.next_page);
    }
  }
  await readPage(null);
  return { operations, unavailable };
}

/** Archives and unreadable entries share one bounded, versioned catalog cursor. */
export async function readArchives(
  remote: { request: (request: GitRequest) => Promise<GitResponse> },
  target: RepositoryTarget,
) {
  const archives: ArchiveReceipt[] = [];
  const unavailable: UnavailableEntry[] = [];
  async function readPage(page: PageCursor | null): Promise<void> {
    const result = await remote.request({
      kind: "archive_list",
      target,
      ...(page === null ? {} : { page }),
    });
    if (result.kind !== "archives") {
      throw new Error("The Git node returned an unexpected archive catalog.");
    }
    archives.push(...result.archives);
    unavailable.push(...result.unavailable);
    const next = result.next_page;
    if (
      next !== null &&
      (next.offset !== archives.length + unavailable.length ||
        next.offset <= (page?.offset ?? 0) ||
        (page !== null && next.digest !== page.digest))
    ) {
      throw new Error(
        "The Git archive catalog changed during recovery. Refresh before continuing.",
      );
    }
    if (next !== null) {
      await readPage(next);
    }
  }
  await readPage(null);
  return { archives, unavailable };
}
