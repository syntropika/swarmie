import type { PlainMessageKey } from "~/src/platform/i18n/catalogs/en";

/** A result supplied by authentication logic for the view to announce. */
export type LoginFeedback = {
  tone: "error" | "success" | "info";
  title: PlainMessageKey;
  message: PlainMessageKey;
};
