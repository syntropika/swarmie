import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { errorMessage, readCheckout } from "./checkout-data";
import type { Checkout, CheckoutSnapshot } from "./checkout-data";
import { readOperations } from "./operation-catalog";
import { pendingKey, readPending, writePending } from "./pending";
import type {
  GitOperation,
  GitReceipt,
  GitRequest,
  GitResponse,
  ManagedMutation,
  GitSettingsValue,
  RepositoryTarget,
} from "./protocol";
import { GitRemote } from "./remote";
import type { GitConnection } from "./remote";

export function useCheckout(connection: GitConnection | null, path: string, projectId?: string) {
  const remote = useMemo(
    () => (connection === null ? null : new GitRemote(connection)),
    [connection],
  );
  const [checkout, setCheckout] = useState<Checkout | null>(null);
  const [fresh, setFresh] = useState(false);
  const freshObservation = useRef(false);
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<GitReceipt | null>(null);
  const [settings, setSettings] = useState<GitSettingsValue | null>(null);
  const [platform, setPlatform] = useState<string | null>(null);
  const [retained, setRetained] = useState({ connection, remote });
  if (connection !== null && connection !== retained.connection) {
    setRetained({ connection, remote });
  }
  const key = pendingKey(connection ?? retained.connection, path);
  const [observedRemote, setObservedRemote] = useState<GitRemote | null>(null);
  const [pendingOperation, setPendingOperation] = useState<string | null>(() => readPending(key));
  const [reviewedOperation, setReviewedOperation] = useState<string | null>(null);
  const epoch = useRef(0);
  const active = useRef(true);
  const mutating = useRef(false);
  const acknowledged = useRef(new Set<string>());
  const unresolved = useRef<GitReceipt[]>([]);
  const [operationWarnings, setOperationWarnings] = useState<string[]>([]);
  const [catalogReady, setCatalogReady] = useState(false);
  const catalogObserved = useRef(false);
  const apply = useCallback((value: CheckoutSnapshot | null) => {
    if (value === null || value.error === null) {
      setCheckout(value?.checkout ?? null);
    }
    const observed = value !== null && value.checkout !== null;
    freshObservation.current = observed;
    setFresh(observed);
    setFailure(value?.error ?? null);
    setSettings(value?.settings ?? null);
    setPlatform(value?.platform ?? null);
  }, []);
  const read = useCallback(async () => {
    const value = await readCheckout(remote, path, projectId);
    if (value?.checkout === null || value === null || remote === null) {
      return { value, catalog: null };
    }
    return { value, catalog: await readOperations(remote, value.checkout.target) };
  }, [remote, path, projectId]);
  const applyRead = useCallback(
    ({ value, catalog }: Awaited<ReturnType<typeof read>>) => {
      if (remote === null) {
        return;
      }
      apply(value);
      setObservedRemote(remote);
      const warnings = catalog?.unavailable.map((entry) => entry.message) ?? [];
      setOperationWarnings(warnings);
      catalogObserved.current = catalog !== null && warnings.length === 0;
      setCatalogReady(catalogObserved.current);
      unresolved.current =
        catalog?.operations.filter(
          (entry) =>
            (entry.state === "unknown" || entry.state === "in_progress") &&
            !acknowledged.current.has(entry.operation_id),
        ) ?? [];
      const [pending] = unresolved.current;
      const operationId = readPending(key) ?? pending?.operation_id ?? null;
      writePending(key, operationId);
      setPendingOperation(operationId);
      if (pending?.operation_id === operationId) {
        setReceipt(pending);
      }
    },
    [apply, key, remote],
  );
  const refresh = useCallback(async () => {
    epoch.current += 1;
    const sequence = epoch.current;
    setBusy(true);
    freshObservation.current = false;
    setFresh(false);
    catalogObserved.current = false;
    setCatalogReady(false);
    setFailure(null);
    try {
      const result = await read();
      if (sequence === epoch.current) {
        applyRead(result);
      }
      return sequence === epoch.current && result.value !== null && result.value.checkout !== null;
    } catch (error: unknown) {
      if (sequence === epoch.current) {
        setFailure(errorMessage(error));
      }
      return false;
    } finally {
      if (sequence === epoch.current) {
        setBusy(false);
        setLoaded(true);
      }
    }
  }, [read, applyRead]);

  useEffect(() => {
    active.current = true;
    freshObservation.current = false;
    catalogObserved.current = false;
    let current = true;
    const sequence = epoch.current;
    async function open() {
      try {
        const result = await read();
        if (current && sequence === epoch.current) {
          applyRead(result);
        }
      } catch (error: unknown) {
        if (current && sequence === epoch.current) {
          setFailure(errorMessage(error));
        }
      } finally {
        if (current && sequence === epoch.current) {
          setLoaded(true);
          setBusy(false);
        }
      }
    }
    open().catch(console.error);
    const requests = epoch;
    const mounted = active;
    return () => {
      current = false;
      requests.current += 1;
      mounted.current = false;
    };
  }, [read, applyRead]);

  async function perform(
    build: (operationId: string, target: RepositoryTarget) => GitRequest,
  ): Promise<GitResponse | null> {
    if (
      remote === null ||
      checkout === null ||
      !freshObservation.current ||
      !catalogObserved.current ||
      busy ||
      readPending(key) !== null ||
      mutating.current
    ) {
      return null;
    }
    const operationId = crypto.randomUUID();
    const sequence = epoch.current;
    const isCurrent = () => active.current && sequence === epoch.current;
    mutating.current = true;
    setBusy(true);
    setFailure(null);
    setReceipt(null);
    try {
      writePending(key, operationId);
      setPendingOperation(operationId);
      const result = await remote.request(build(operationId, checkout.target));
      if (!isCurrent()) {
        return null;
      }
      if ("receipt" in result) {
        if (isCurrent()) {
          setReceipt(result.receipt);
        }
        if (result.receipt.state === "unknown" || result.receipt.state === "in_progress") {
          return result;
        }
      }
      writePending(key, null);
      if (isCurrent()) {
        setPendingOperation(null);
        await refresh();
      }
      return active.current && connection !== null && connection.isCurrent() ? result : null;
    } catch (error: unknown) {
      if (isCurrent()) {
        setFailure(errorMessage(error));
      }
      return null;
    } finally {
      mutating.current = false;
      if (isCurrent()) {
        setBusy(false);
      }
    }
  }

  async function mutate(operation: GitOperation) {
    const result = await perform((operationId, target) => ({
      kind: "mutate",
      operation_id: operationId,
      target,
      operation,
    }));
    return result?.kind === "receipt" && result.receipt.state === "completed";
  }

  async function managed(mutation: ManagedMutation) {
    return await perform((operationId, target) => ({
      ...mutation,
      operation_id: operationId,
      target,
    }));
  }

  async function recover() {
    if (remote === null || pendingOperation === null || busy) {
      return;
    }
    setBusy(true);
    const sequence = epoch.current;
    setFailure(null);
    try {
      const result = await remote.request({ kind: "recover", operation_id: pendingOperation });
      if (!active.current || sequence !== epoch.current) {
        return;
      }
      if (
        result.kind !== "receipt" ||
        result.receipt.operation_id !== pendingOperation ||
        result.receipt.principal !== remote.identity.principal
      ) {
        throw new Error("The Git node returned an unexpected recovery receipt.");
      }
      setReceipt(result.receipt);
      if (result.receipt.state !== "unknown" && result.receipt.state !== "in_progress") {
        writePending(key, null);
        setPendingOperation(null);
        await refresh();
      }
    } catch (error: unknown) {
      if (active.current && sequence === epoch.current) {
        setFailure(errorMessage(error));
      }
    } finally {
      if (active.current && sequence === epoch.current) {
        setBusy(false);
      }
    }
  }

  async function reviewUnknown() {
    if (receipt?.state !== "unknown" || receipt.operation_id !== pendingOperation) {
      return;
    }
    setReviewedOperation(null);
    if (await refresh()) {
      setReviewedOperation(receipt.operation_id);
    }
  }

  function acknowledgeUnknown() {
    if (
      receipt?.state !== "unknown" ||
      receipt.operation_id !== pendingOperation ||
      reviewedOperation !== pendingOperation
    ) {
      return;
    }
    try {
      acknowledged.current.add(pendingOperation);
      const next = unresolved.current.find(
        (entry) => !acknowledged.current.has(entry.operation_id),
      );
      writePending(key, next?.operation_id ?? null);
      setPendingOperation(next?.operation_id ?? null);
      if (next !== undefined) {
        setReceipt(next);
      }
      setReviewedOperation(null);
    } catch (error: unknown) {
      setFailure(errorMessage(error));
    }
  }

  return {
    remote: remote ?? retained.remote,
    connected: connection !== null && connection.isCurrent(),
    checkout,
    fresh:
      fresh &&
      catalogReady &&
      observedRemote === remote &&
      connection !== null &&
      connection.isCurrent(),
    operationWarnings,
    busy,
    loaded,
    error: failure,
    receipt,
    pendingOperation,
    refresh,
    mutate,
    managed,
    recover,
    reviewUnknown,
    acknowledgeUnknown,
    unknownReviewed: reviewedOperation !== null && reviewedOperation === pendingOperation,
    settings,
    platform,
    fail: (error: unknown) => {
      setFailure(errorMessage(error));
    },
  };
}
