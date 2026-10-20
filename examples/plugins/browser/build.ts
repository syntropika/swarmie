import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "../../..");
const source = path.join(root, "examples/plugins/browser");
const external = ["@tembra/sdk", "@tembra/sdk/browser", "react", "react-dom", "react-dom/client"];

/** Only the application-owned contract module is shared; native module evaluation stays in Tembra. */
export async function bundle(entry: string, sharedContracts = false): Promise<string> {
  let excluded = external;
  if (entry === "sdk.ts" || entry === "react.ts") {
    excluded = [];
  }
  const result = await Bun.build({
    entrypoints: [path.join(source, entry)],
    target: "browser",
    format: "esm",
    external: excluded,
    plugins: sharedContracts
      ? [
          {
            name: "shared-contracts",
            setup(builder) {
              builder.onResolve({ filter: /^\.\/contracts$/u }, () => ({
                path: "swarmie.acceptance.contracts",
                external: true,
              }));
            },
          },
        ]
      : [],
  });
  const [output] = result.outputs;
  if (!result.success || output === undefined) {
    throw new Error(result.logs.map(String).join("\n"));
  }
  return await output.text();
}

function digest(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

export async function packageBrowserFixtures(): Promise<void> {
  const provenance: unknown = JSON.parse(
    await readFile(path.join(root, "packages/tembra-sdk/provenance.json"), "utf-8"),
  );
  if (
    provenance === null ||
    typeof provenance !== "object" ||
    !("revision" in provenance) ||
    typeof provenance.revision !== "string"
  ) {
    throw new Error("SDK revision provenance is missing");
  }
  const artifacts: unknown = JSON.parse(
    await readFile(path.join(root, "target/plugins/artifacts.json"), "utf-8"),
  );
  if (artifacts === null || typeof artifacts !== "object") {
    throw new Error("Server artifact inventory is missing");
  }
  await Promise.all(
    [
      ["1.0.0", "v1.ts", "dynamic-consumer-v1"],
      ["1.1.0", "v2.ts", "dynamic-consumer-v2"],
    ].map(async ([version, entry, server]) => {
      if (version === undefined || entry === undefined || server === undefined) {
        throw new TypeError("Invalid fixture version");
      }
      const artifact: unknown = Reflect.get(artifacts, server);
      if (typeof artifact !== "string") {
        throw new TypeError("Required server artifact is missing");
      }
      // Each package retains exact runtime inputs; no generated file mutates an approved version.
      const files = {
        "server.json": await readFile(path.join(root, artifact)),
        "facet.js": await bundle(entry, true),
        "widget.js": await bundle("widget.ts"),
      };
      const manifest = {
        schema_version: 1,
        id: "multi-host-widget",
        version,
        compatibility: { swarmie: "^0.1.0", tembra_revision: provenance.revision },
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

if (import.meta.main) {
  await packageBrowserFixtures();
}
