import { de } from "~/src/platform/i18n/catalogs/de";
import { en } from "~/src/platform/i18n/catalogs/en";
import type { MessageKey, Messages } from "~/src/platform/i18n/catalogs/en";
import { es } from "~/src/platform/i18n/catalogs/es";
import { fr } from "~/src/platform/i18n/catalogs/fr";
import { it } from "~/src/platform/i18n/catalogs/it";
import { ja } from "~/src/platform/i18n/catalogs/ja";
import { pt } from "~/src/platform/i18n/catalogs/pt";
import { zhCN } from "~/src/platform/i18n/catalogs/zh-cn";
import type { Locale } from "~/src/platform/i18n/locale";

export const catalogs: Record<Locale, Messages> = { en, es, fr, de, pt, it, ja, "zh-CN": zhCN };
type Slots<Value extends string> = Value extends `${string}{${infer Name}}${infer Rest}`
  ? Name | Slots<Rest>
  : never;
type Arguments<Key extends MessageKey> = [Slots<(typeof en)[Key]>] extends [never]
  ? []
  : [values: Record<Slots<(typeof en)[Key]>, string | number>];
export type Translate = <Key extends MessageKey>(key: Key, ...args: Arguments<Key>) => string;

/** Typed keys and complete interpolation; translated values are always rendered as text. */
export function createTranslator(locale: Locale): Translate {
  return (key, ...args) => {
    const values: Record<string, string | number> | undefined = args[0];
    return catalogs[locale][key].replaceAll(/\{(?<name>[^}]+)\}/gu, (slot: string, name: string) =>
      values?.[name] === undefined ? slot : String(values[name]),
    );
  };
}

const statusKeys = {
  idle: "status.idle",
  running: "status.running",
  closed: "status.closed",
  failed: "status.failed",
  scheduled: "status.scheduled",
  interrupted: "status.interrupted",
  cancelled: "status.cancelled",
  delivered: "status.delivered",
  dispatching: "status.dispatching",
  error: "status.error",
  needs_input: "status.needs_input",
  cancelling: "status.cancelling",
} as const satisfies Record<string, MessageKey>;

export function statusLabel(t: Translate, status: string | undefined): string {
  for (const [value, key] of Object.entries(statusKeys)) {
    if (value === status) {
      return t(key);
    }
  }
  return t("status.unknown");
}
