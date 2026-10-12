// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import type { RuntimeEvent, SessionSnapshot, SubmissionReceipt } from "./index.js";

/** Native application projection; receipt ownership does not depend on view caches. */
export type SubmissionProjection = {
  entries: { key: string; sessionId: string; receipt: SubmissionReceipt }[];
  unresolved: boolean;
  snapshot: SessionSnapshot;
};
export type SessionProjection = SubmissionProjection & { events: RuntimeEvent[] };
export interface NativeClientMethods {
  "client/sessionProjection": {
    params: { sessionId: string; afterSequence?: number };
    result: SessionProjection;
  };
  "client/submissions": {
    params: { sessionId: string; reconcile?: boolean };
    result: SubmissionProjection;
  };
}
