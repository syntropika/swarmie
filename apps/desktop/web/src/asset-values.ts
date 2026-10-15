import { record } from "./checked-json";

export type PreviewAsset = {
  name: string;
  mime: string;
  data: string;
  size?: number;
  path?: string;
};
export function decodeAsset(value: unknown): PreviewAsset {
  const asset = record(value);
  if (
    typeof asset.name !== "string" ||
    typeof asset.mime !== "string" ||
    typeof asset.data !== "string"
  ) {
    throw new TypeError("The file preview is invalid.");
  }
  return {
    name: asset.name,
    mime: asset.mime,
    data: asset.data,
    ...(typeof asset.size === "number" ? { size: asset.size } : {}),
    ...(typeof asset.path === "string" ? { path: asset.path } : {}),
  };
}
export function decodeExport(value: unknown) {
  const result = record(value);
  if (typeof result.path !== "string") {
    throw new TypeError("The saved file path is invalid.");
  }
  return { path: result.path };
}
