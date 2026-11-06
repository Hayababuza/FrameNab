import de from "./de.json";
import en from "./en.json";

export const dictionaries = { de, en } as const;
export type Locale = keyof typeof dictionaries;
export type MessageKey = keyof typeof de;
export const locales = Object.keys(dictionaries) as Locale[];

/** Picks the matching locale for a browser/system language tag such as `de-AT`. */
export function resolveLocale(tag: string | undefined | null): Locale {
  const base = (tag ?? "").toLowerCase().split(/[-_]/)[0];
  return (locales as string[]).includes(base) ? (base as Locale) : "en";
}

/** Translates `key` and fills placeholders such as `{phase}`. Falls back to English, then to the key. */
export function translate(
  locale: Locale,
  key: MessageKey,
  params: Record<string, string | number> = {},
): string {
  const text: string = dictionaries[locale][key] ?? dictionaries.en[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}
