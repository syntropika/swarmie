import type { ComponentType } from "react";

/** Presentation state supplied by the host; it does not own task execution. */
export type PartStatus =
  | { type: "running" }
  | { type: "complete" }
  | { type: "incomplete"; reason: "cancelled" | "content-filter" | "error" | "length" | "other" };

export type ImageMessagePart = { image: string; filename?: string | undefined; type?: "image" };
export type ImageMessagePartComponent = ComponentType<
  ImageMessagePart & { status?: PartStatus | undefined }
>;
export type FileMessagePartComponent = ComponentType<{
  type?: "file";
  data: string;
  mimeType: string;
  filename?: string | undefined;
  sourceType?: "url" | "id" | undefined;
  status?: PartStatus | undefined;
}>;
