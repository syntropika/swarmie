import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

import type { BuiltArtifact } from "@tembra/sdk/tooling";

import type { RemoteBrowserAccessMainActivePackageResult } from "../../target/generated/contracts/remote-browser-access";
import { bundleModule } from "../../tests/plugins/browser/build";

type PluginManifest = RemoteBrowserAccessMainActivePackageResult["manifest"];

const root = path.resolve(import.meta.dirname, "../..");
function digest(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

async function writeJson(file: string, value: unknown): Promise<void> {
  await writeFile(file, `${JSON.stringify(value, null, 2)}\n`);
}

/** Swarmie release metadata composes actual upstream artifacts without rebuilding them. */
export async function packageFixtures(artifacts: readonly BuiltArtifact[]): Promise<void> {
  const releases = [
    ["dynamic-consumer-v1", "1.0.0"],
    ["dynamic-consumer-v2", "1.1.0"],
    ["dynamic-consumer-failing", "1.2.0"],
    ["dynamic-consumer-provider-removed", "1.3.0"],
    ["dynamic-consumer-evaluation-failure", "1.4.0"],
    ["dynamic-consumer-invalid-result", "1.5.0"],
    ["dynamic-consumer-invalid-argument", "1.6.0"],
    ["dynamic-consumer-incompatible", "1.7.0"],
    ["alternative-counter", "1.0.0"],
  ] as const;
  const provenance: unknown = JSON.parse(
    await readFile(path.join(root, "packages/tembra-sdk/provenance.json"), "utf-8"),
  );
  if (
    provenance === null ||
    typeof provenance !== "object" ||
    !("revision" in provenance) ||
    typeof provenance.revision !== "string"
  ) {
    throw new TypeError("SDK provenance is missing");
  }
  const { revision } = provenance;
  await Promise.all(
    releases.map(async ([target, version]) => {
      const artifact = artifacts.find((candidate) => candidate.target === target);
      if (artifact === undefined) {
        throw new Error(`Required artifact is missing: ${target}`);
      }
      const plugin = target === "alternative-counter" ? target : "dynamic-consumer";
      const destination = path.join(root, "target/fixtures", plugin, version);
      await mkdir(destination, { recursive: true });
      const bytes = await readFile(artifact.artifactPath);
      await writeFile(path.join(destination, "artifact.json"), bytes);
      const sha256 = digest(bytes);
      const manifest: PluginManifest = {
        schema_version: 1,
        id: plugin,
        version,
        compatibility: { swarmie: "^0.1.0", tembra_revision: revision },
        dependencies: [],
        facets: [
          {
            id: plugin,
            target: "server",
            format: "bundled",
            artifact_path: "artifact.json",
            sha256,
            selected_export: artifact.selectedExport,
          },
        ],
        files: [{ path: "artifact.json", sha256 }],
        distribution: {
          provider: "local",
          description: "Contract-based Counter composition acceptance fixture",
        },
        ui: [],
      };
      await writeJson(path.join(destination, "manifest.json"), manifest);
    }),
  );
  const inventory = Object.fromEntries(
    artifacts.map((artifact) => [artifact.target, path.relative(root, artifact.artifactPath)]),
  );
  await writeJson(path.join(root, "target/plugins/artifacts.json"), inventory);
  await packageBrowserFixtures(artifacts, revision);
}

async function packageBrowserFixtures(
  artifacts: readonly BuiltArtifact[],
  revision: string,
): Promise<void> {
  await Promise.all(
    [
      ["1.0.0", "v1.ts", "dynamic-consumer-v1"],
      ["1.1.0", "v2.ts", "dynamic-consumer-v2"],
    ].map(async ([version, entry, server]) => {
      if (version === undefined || entry === undefined || server === undefined) {
        throw new TypeError("Invalid fixture version");
      }
      const artifact = artifacts.find((candidate) => candidate.target === server);
      if (artifact === undefined) {
        throw new TypeError("Required server artifact is missing");
      }
      // Each package retains exact runtime inputs; no generated file mutates an approved version.
      const files = {
        "server.json": await readFile(artifact.artifactPath),
        "facet.js": await bundleModule(
          `examples/plugins/browser-widget/${entry === "v1.ts" ? "index.ts" : "v2.ts"}`,
          { sharedContracts: true },
        ),
        "widget.js": await bundleModule("examples/plugins/browser-widget/widget.ts"),
      };
      const manifest: PluginManifest = {
        schema_version: 1,
        id: "multi-host-widget",
        version,
        compatibility: { swarmie: "^0.1.0", tembra_revision: revision },
        dependencies: [],
        facets: [
          {
            id: "dynamic-consumer",
            target: "server",
            format: "bundled",
            artifact_path: "server.json",
            sha256: digest(files["server.json"]),
            selected_export: "default",
          },
          {
            id: "browser-widget",
            target: "browser",
            format: "native_esm",
            artifact_path: "facet.js",
            sha256: digest(files["facet.js"]),
            selected_export: "default",
          },
        ],
        files: Object.entries(files).map(([name, value]) => ({
          path: name,
          sha256: digest(value),
        })),
        distribution: {
          provider: "local",
          description: "Server and Browser Host acceptance fixture",
        },
        ui: [
          {
            id: "widget",
            plugin_id: "multi-host-widget",
            kind: "panel",
            label: `Counter view ${entry === "v1.ts" ? 1 : 2}`,
            slot_id: "acceptance",
            order: 0,
            module_path: "widget.js",
            export_name: "Widget",
          },
        ],
      };
      const destination = path.join(root, "target/fixtures/multi-host-widget", version);
      await mkdir(destination, { recursive: true });
      await Promise.all(
        Object.entries(files).map(async ([name, value]) => {
          await writeFile(path.join(destination, name), value);
        }),
      );
      await writeFile(
        path.join(destination, "manifest.json"),
        `${JSON.stringify(manifest, null, 2)}\n`,
      );
    }),
  );
}
