/** Reserved route boundary. No adapter or permissions are activated in this release. */
export type ComputerSessionCapability =
  | { availability: "unavailable" }
  | {
      availability: "available";
      sessionId: string;
      capabilities: ReadonlySet<"display" | "input" | "clipboard" | "takeover" | "return">;
    };

export type ComputerSessionAdapter = {
  inspect: (sessionId: string, signal: AbortSignal) => Promise<ComputerSessionCapability>;
  open: (
    capability: Extract<ComputerSessionCapability, { availability: "available" }>,
    signal: AbortSignal,
  ) => Promise<{ close: () => Promise<void> }>;
};
