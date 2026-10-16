/** Explicit URL imports keep the shared artwork contract independent of application tooling types. */
declare module "*.svg?url" {
  const source: string;
  export default source;
}
