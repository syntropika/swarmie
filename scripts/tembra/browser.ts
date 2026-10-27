import { cp, mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Export the installed browser ABI; package installation owns its integrity.
const source = path.dirname(
  fileURLToPath(import.meta.resolve("@syntropika/tembra-engine-wasm/tembra_bg.wasm")),
);
const destination = path.join(process.cwd(), "target", "browser-wasm");
await mkdir(path.dirname(destination), { recursive: true });
await rm(destination, { recursive: true, force: true });
await cp(source, destination, { recursive: true });
