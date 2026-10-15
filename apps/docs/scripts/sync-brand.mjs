import { readFile, writeFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const source = path.resolve(root, "../../assets/brandkit");
const destination = path.join(root, "public/brand");
const check = process.argv.includes("--check");
const assets = [
  ["logos/swarmie-mono-ink.svg", "public/brand/swarmie-mono-ink.svg"],
  ["logos/swarmie-mono-white.svg", "public/brand/swarmie-mono-white.svg"],
  ["marks/swarmie-mark-mono-ink.svg", "public/brand/swarmie-mark-mono-ink.svg"],
  ["marks/swarmie-mark-mono-white.svg", "public/brand/swarmie-mark-mono-white.svg"],
  ["tokens/colors.css", "app/brand-tokens.css"],
];

if (!check) {
  await mkdir(destination, { recursive: true });
}
await Promise.all(
  assets.map(async ([asset, output]) => {
    const expected = await readFile(path.join(source, asset));
    const target = path.join(root, output);
    if (check) {
      const actual = await readFile(target).catch(() => Buffer.alloc(0));
      if (!actual.equals(expected)) {
        throw new Error(
          `Documentation brand asset is stale: ${asset}. Run node scripts/sync-brand.mjs.`,
        );
      }
    } else {
      await writeFile(target, expected);
    }
  }),
);
console.log(`${assets.length} official brand assets ${check ? "checked" : "synchronized"}.`);
