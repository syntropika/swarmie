import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { errorMessage, readCheckout } from "./checkout-data";
import type { Checkout, CheckoutSnapshot } from "./checkout-data";
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
  const key = pendingKey(connection, path);
  const [pendingOperation, setPendingOperation] = useState<string | null>(() => readPending(key));
  const [reviewedOperation, setReviewedOperation] = useState<string | null>(null);
  const epoch = useRef(0);
  const active = useRef(true);
  const mutating = useRef(false);
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
  const refresh = useCallback(async () => {
    epoch.current += 1;
    const sequence = epoch.current;
    setBusy(true);
    freshObservation.current = false;
    setFresh(false);
    setFailure(null);
    try {
      const value = await readCheckout(remote, path, projectId);
      if (sequence === epoch.current) {
        apply(value);
      }
      return sequence === epoch.current && value !== null && value.checkout !== null;
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
  }, [remote, path, projectId, apply]);

  useEffect(() => {
    active.current = true;
    let current = true;
    const sequence = epoch.current;
    async function open() {
      try {
        const value = await readCheckout(remote, path, projectId);
        if (current && sequence === epoch.current) {
          apply(value);
        }
      } catch (error: unknown) {
        if (current && sequence === epoch.current) {
          setFailure(errorMessage(error));
        }
      } finally {
        if (current && sequence === epoch.current) {
          setLoaded(true);
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
  }, [remote, path, projectId, apply]);

  async function perform(
    build: (operationId: string, target: RepositoryTarget) => GitRequest,
  ): Promise<GitResponse | null> {
    if (
      remote === null ||
      checkout === null ||
      !freshObservation.current ||
      busy ||
      readPending(key) !== null ||
      mutating.current
    ) {
      return null;
    }
    const operationId = crypto.randomUUID();
    mutating.current = true;
    setBusy(true);
    setFailure(null);
    setReceipt(null);
    try {
      writePending(key, operationId);
      setPendingOperation(operationId);
      const result = await remote.request(build(operationId, checkout.target));
      if ("receipt" in result) {
        if (active.current) {
          setReceipt(result.receipt);
        }
        if (result.receipt.state === "unknown") {
          return result;
        }
      }
      writePending(key, null);
      if (active.current) {
        setPendingOperation(null);
        await refresh();
      }
      return active.current ? result : null;
    } catch (error: unknown) {
      setFailure(errorMessage(error));
      return null;
    } finally {
      mutating.current = false;
      if (active.current) {
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
    setFailure(null);
    try {
      const result = await remote.request({ kind: "recover", operation_id: pendingOperation });
      if (
        result.kind !== "receipt" ||
        result.receipt.operation_id !== pendingOperation ||
        result.receipt.principal !== remote.identity.principal
      ) {
        throw new Error("The Git node returned an unexpected recovery receipt.");
      }
      setReceipt(result.receipt);
      if (result.receipt.state !== "unknown") {
        writePending(key, null);
        setPendingOperation(null);
        await refresh();
      }
    } catch (error: unknown) {
      setFailure(errorMessage(error));
    } finally {
      setBusy(false);
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
      writePending(key, null);
      setPendingOperation(null);
      setReviewedOperation(null);
    } catch (error: unknown) {
      setFailure(errorMessage(error));
    }
  }

  return {
    remote,
    checkout,
    fresh,
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
