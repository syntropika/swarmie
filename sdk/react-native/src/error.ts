export class NativeClientError extends Error {
  constructor(public readonly kind: string) {
    super(`Runtime operation failed (${kind}).`);
    this.name = "NativeClientError";
  }
}
