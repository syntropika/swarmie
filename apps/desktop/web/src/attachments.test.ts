import { describe, expect, it, vi } from "vitest";

import { attachmentBytes, attachmentLimit, readAttachment } from "~/web/src/attachments";

function file(bytes: number[] | string, name = "context.txt", type = "text/plain"): File {
  return new File([typeof bytes === "string" ? bytes : new Uint8Array(bytes)], name, { type });
}
describe("attachment validation", () => {
  it.each(["application/pdf", "audio/wav", "video/mp4", "application/octet-stream"])(
    "preserves %s bytes without interpreting them as text",
    async (mime) => {
      const attachment = await readAttachment(file([0, 255, 42], "media", mime));
      expect(attachment).toMatchObject({ binary: true, image: false, mime });
      expect(Array.from(atob(attachment.data), (character) => character.codePointAt(0))).toEqual([
        0, 255, 42,
      ]);
      expect(attachmentBytes(attachment)).toBe(3);
    },
  );
  it("counts padded images at the supported 8 MiB boundary exactly", () => {
    for (const size of [attachmentLimit - 1, attachmentLimit, attachmentLimit + 1]) {
      const data = btoa("\0".repeat(size));
      expect(
        attachmentBytes({
          id: "image",
          name: "boundary.png",
          mime: "image/png",
          image: true,
          data,
        }),
      ).toBe(size);
    }
  });
  it("keeps UTF-8 file context explicit", async () => {
    expect(await readAttachment(file("Project context"))).toMatchObject({
      name: "context.txt",
      data: "Project context",
      image: false,
    });
  });
  it.each(["", "application/zip", "application/vnd.custom-format"])(
    "preserves opaque bytes with MIME %s",
    async (mime) => {
      const attachment = await readAttachment(file([0, 255, 42], "opaque.bin", mime));
      expect(attachment).toMatchObject({
        binary: true,
        image: false,
        mime: mime || "application/octet-stream",
        data: "AP8q",
      });
    },
  );
  it("reads an untyped file as text when its bytes are valid UTF-8", async () => {
    expect(await readAttachment(file("Readable source", "source.custom", ""))).toMatchObject({
      data: "Readable source",
      mime: "text/plain",
      image: false,
    });
  });
  it("rejects binary and invalid UTF-8 data rather than corrupting a prompt", async () => {
    await expect(readAttachment(file([0, 42]))).rejects.toThrow("binary");
    await expect(readAttachment(file([255, 254]))).rejects.toThrow(/utf-8/iu);
  });
  it("bounds file size before reading its payload", async () => {
    const large = new File([new Uint8Array(attachmentLimit + 1)], "too-large.txt", {
      type: "text/plain",
    });
    const read = vi.spyOn(large, "arrayBuffer");
    await expect(readAttachment(large)).rejects.toThrow("8 MB");
    expect(read).not.toHaveBeenCalled();
  });
  it("refuses unsupported image types", async () => {
    await expect(readAttachment(file("svg", "diagram.svg", "image/svg+xml"))).rejects.toThrow(
      "PNG, JPEG",
    );
  });
});
