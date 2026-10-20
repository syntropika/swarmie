import path from "node:path";

const root = path.resolve(import.meta.dirname, "../../..");
const external = ["@tembra/sdk", "@tembra/sdk/browser", "react", "react-dom", "react-dom/client"];

/** The test embedding shares ordinary contract modules; Tembra owns native ESM execution. */
export async function bundleModule(
  entry: string,
  options: { readonly sharedContracts?: boolean; readonly includeDependencies?: boolean } = {},
): Promise<string> {
  const result = await Bun.build({
    entrypoints: [path.join(root, entry)],
    target: "browser",
    format: "esm",
    external: options.includeDependencies === true ? [] : external,
    plugins:
      options.sharedContracts === true
        ? [
            {
              name: "shared-contracts",
              setup(builder) {
                builder.onResolve({ filter: /(?:^|\/)contracts$/u }, () => ({
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
