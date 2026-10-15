import { z } from "zod";

export const connectionSchema = z.object({
  url: z.url().refine((value) => {
    try {
      const url = new URL(value);
      return (
        url.protocol === "https:" && !url.username && !url.password && !url.search && !url.hash
      );
    } catch {
      // Zod can run refinements after a format issue; an empty form must stay renderable.
      return false;
    }
  }, "Use an HTTPS runtime URL without credentials, query or fragment."),
  token: z.string().trim().min(1).max(4096),
});

export type ConnectionSettings = z.infer<typeof connectionSchema>;
