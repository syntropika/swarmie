/// <reference types="vite/client" />

const sources = import.meta.glob<string>("./examples/*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
});

export function officialExampleSource(name: string) {
  const source = sources[`./examples/${name}.tsx`];
  if (source === undefined) {
    throw new Error(`Missing official example source: ${name}`);
  }
  return source;
}
