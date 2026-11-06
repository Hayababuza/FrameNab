import de from "./de.json";
import en from "./en.json";

export const dictionaries = { de, en } as const;
export type Locale = keyof typeof dictionaries;
export type MessageKey = keyof typeof de;
export const locales = Object.keys(dictionaries) as Locale[];

/** Wählt die passende Sprache aus einer Browser-/System-Sprachangabe wie `de-AT`. */
export function resolveLocale(tag: string | undefined | null): Locale {
  const base = (tag ?? "").toLowerCase().split(/[-_]/)[0];
  return (locales as string[]).includes(base) ? (base as Locale) : "en";
}

/** Übersetzt `key` und ersetzt Platzhalter wie `{phase}`. Fällt auf Englisch, dann auf den Schlüssel zurück. */
export function translate(
  locale: Locale,
  key: MessageKey,
  params: Record<string, string | number> = {},
): string {
  const text: string = dictionaries[locale][key] ?? dictionaries.en[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}
