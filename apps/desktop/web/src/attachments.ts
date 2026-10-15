import type { Attachment } from "~/web/src/protocol";

export const attachmentLimit = 8 * 1024 * 1024;
export function attachmentBytes(attachment: Attachment): number {
  if (!attachment.image && !(attachment.binary === true)) {
    return new TextEncoder().encode(attachment.data).length;
  }
  const padding = (() => {
    if (attachment.data.endsWith("==")) {
      return 2;
    }
    if (attachment.data.endsWith("=")) {
      return 1;
    }
    return 0;
  })();
  return Math.floor((attachment.data.length * 3) / 4) - padding;
}
function encode(bytes: Uint8Array) {
  const parts = [];
  for (let index = 0; index < bytes.length; index += 16_384) {
    parts.push(String.fromCodePoint(...bytes.subarray(index, index + 16_384)));
  }
  return btoa(parts.join(""));
}

export async function readAttachment(file: File): Promise<Attachment> {
  if (file.size > attachmentLimit) {
    throw new Error(`${file.name} exceeds the 8 MB attachment limit.`);
  }
  const image = ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(file.type);
  let binary =
    file.type === "application/pdf" ||
    file.type.startsWith("audio/") ||
    file.type.startsWith("video/") ||
    file.type === "application/octet-stream";
  if (file.type.startsWith("image/") && !image) {
    throw new Error("Use a PNG, JPEG, GIF or WebP image.");
  }
  let data: string;
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (image || binary) {
    data = encode(bytes);
  } else {
    try {
      if (bytes.includes(0)) {
        throw new Error(`${file.name} contains binary data.`);
      }
      data = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
      if (file.type.startsWith("text/")) {
        throw new Error(
          bytes.includes(0)
            ? `${file.name} contains binary data and cannot be used as text.`
            : `${file.name} is not UTF-8 text.`,
        );
      }
      binary = true;
      data = encode(bytes);
    }
  }
  return {
    id: crypto.randomUUID(),
    name: file.name,
    mime: file.type || (binary ? "application/octet-stream" : "text/plain"),
    data,
    image,
    ...(binary ? { binary: true } : {}),
  };
}
