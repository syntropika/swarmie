import { readFile, writeFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { format } from "oxfmt";

import formatting from "../../../oxfmt.config.ts";

const docsRoot = fileURLToPath(new URL("../", import.meta.url));
const repository = path.resolve(docsRoot, "../..");
const check = process.argv.includes("--check");
const schemaText = await readFile(
  path.join(repository, "packages/contracts/protocol.json"),
  "utf-8",
);
const schema = JSON.parse(schemaText);
function summary(value) {
  if (value.$ref) {
    return value.$ref.split("/").at(-1);
  }
  if (value.type === "array") {
    return `${summary(value.items)}[]`;
  }
  return value.type ?? "JSON value";
}
const groups = Map.groupBy(
  Object.entries(schema["x-methods"]).toSorted(([a], [b]) => a.localeCompare(b)),
  ([name]) => name.split("/")[0],
);
let page =
  '---\ntitle: "Protocol methods"\ndescription: "The method catalog generated from the canonical public runtime schema."\n---\n\nThis reference is generated from `packages/contracts/protocol.json`. Parameter and result details are available in the [downloadable schema](/contracts/protocol.json). Availability of a method does not imply that every harness supports its native operation.\n';
for (const [group, methods] of groups) {
  page += `\n## ${group}\n\n| Method | Parameters | Result |\n| --- | --- | --- |\n`;
  for (const [name, definition] of methods) {
    page += `| \`${name}\` | \`${summary(definition.params)}\` | \`${summary(definition.result)}\` |\n`;
  }
}
const formattedPage = await format("content/docs/reference/protocol-methods.mdx", page, formatting);
if (formattedPage.errors.length > 0) {
  throw new Error("Public protocol reference could not be formatted");
}
const files = new Map([
  ["content/docs/reference/protocol-methods.mdx", formattedPage.code],
  ["public/contracts/protocol.json", schemaText],
]);
await Promise.all(
  [...files].map(async ([name, content]) => {
    const target = path.join(docsRoot, name);
    if (check) {
      const actual = await readFile(target, "utf-8").catch(() => "");
      if (actual !== content) {
        throw new Error(`Public reference is stale: ${name}. Run node scripts/sync-reference.mjs.`);
      }
    } else {
      await mkdir(path.dirname(target), { recursive: true });
      await writeFile(target, content);
    }
  }),
);
console.log(
  `${Object.keys(schema["x-methods"]).length} public methods ${check ? "checked" : "synchronized"}.`,
);
