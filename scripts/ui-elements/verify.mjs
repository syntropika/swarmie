/** Verify the actual public browser module graph, including transitive dependency code. */
const result = await Bun.build({
  entrypoints: ["./packages/ui/web/src/elements/index.ts"],
  target: "browser",
  metafile: true,
  loader: { ".svg": "dataurl" },
});
if (!result.success) {
  throw new AggregateError(result.logs, "The public Elements graph failed to build");
}
const inputs = Object.keys(result.metafile.inputs);
const forbidden = inputs.filter((file) =>
  /@assistant-ui(?:\+|\/)(?:react(?:@|\/)|core(?:@|\/)|store(?:@|\/))|\.aui(?:\.radix)?\.[jt]sx?$/u.test(
    file,
  ),
);
if (forbidden.length) {
  throw new Error(
    `Forbidden assistant runtime dependencies in the public UI graph: ${forbidden.join(", ")}`,
  );
}
console.log(JSON.stringify({ success: true, modules: inputs.length, assistantRuntimeModules: 0 }));
