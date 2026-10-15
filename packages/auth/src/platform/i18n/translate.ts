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

type Placeholders<Value extends string> = Value extends `${string}{${infer Name}}${infer Rest}`
  ? Name | Placeholders<Rest>
  : never;
type Parameters<Key extends MessageKey> = Record<Placeholders<(typeof en)[Key]>, string | number>;
type Arguments<Key extends MessageKey> = [Placeholders<(typeof en)[Key]>] extends [never]
  ? []
  : [parameters: Parameters<Key>];

/** Keys and interpolation arguments are checked at the call site; values remain React text. */
export type Translate = <Key extends MessageKey>(key: Key, ...arguments_: Arguments<Key>) => string;

export function createTranslator(locale: Locale): Translate {
  return (key, ...arguments_) => {
    const parameters: Record<string, string | number> | undefined = arguments_[0];
    return catalogs[locale][key].replaceAll(
      /\{(?<name>[^}]+)\}/gu,
      (placeholder: string, name: string) => {
        const value = parameters?.[name];
        return value === undefined ? placeholder : String(value);
      },
    );
  };
}
