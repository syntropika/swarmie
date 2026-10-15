import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const docs = fileURLToPath(new URL("../", import.meta.url));
const repository = path.resolve(docs, "../..");
const { scripts } = JSON.parse(await readFile(path.join(docs, "package.json"), "utf-8"));

for (const asset of ["public/contracts/protocol.json", "public/brand/swarmie-mono-ink.svg"]) {
  test(`build rejects stale ${asset} without rewriting it`, async () => {
    const fixture = await mkdtemp(path.join(tmpdir(), "swarmie-docs-generated-"));
    const target = path.join(fixture, "apps/docs");
    try {
      await mkdir(target, { recursive: true });
      await Promise.all(
        ["scripts", "content", "public", "src"].map((folder) =>
          cp(path.join(docs, folder), path.join(target, folder), { recursive: true }),
        ),
      );
      await cp(path.join(repository, "assets"), path.join(fixture, "assets"), { recursive: true });
      await mkdir(path.join(fixture, "packages/contracts"), { recursive: true });
      await cp(
        path.join(repository, "packages/contracts/protocol.json"),
        path.join(fixture, "packages/contracts/protocol.json"),
      );
      await cp(path.join(repository, "oxfmt.config.ts"), path.join(fixture, "oxfmt.config.ts"));
      await symlink(
        path.join(repository, "node_modules"),
        path.join(fixture, "node_modules"),
        "dir",
      );
      const clean = spawnSync(scripts.prebuild, { cwd: target, shell: true, encoding: "utf-8" });
      assert.equal(clean.status, 0, clean.stdout + clean.stderr);
      // The real prebuild must reject stale committed output before any generation.
      const file = path.join(target, asset);
      const stale = `${await readFile(file, "utf-8")}\n`;
      await writeFile(file, stale);
      const result = spawnSync(scripts.prebuild, { cwd: target, shell: true, encoding: "utf-8" });
      assert.notEqual(result.status, 0, "prebuild accepted stale generated output");
      assert.match(result.stdout + result.stderr, /stale/iu);
      assert.equal(
        await readFile(file, "utf-8"),
        stale,
        "prebuild silently repaired the stale file",
      );
    } finally {
      await rm(fixture, { recursive: true, force: true });
    }
  });
}
