import { type Locale, type MessageKey, resolveLocale, translate } from "./translate";

export { locales, type Locale } from "./translate";

const state = $state<{ locale: Locale }>({ locale: resolveLocale(globalThis.navigator?.language) });

export const i18n = {
  get locale(): Locale {
    return state.locale;
  },
  /** `null` = system language */
  set(locale: Locale | null) {
    state.locale = locale ?? resolveLocale(globalThis.navigator?.language);
    document.documentElement.lang = state.locale;
  },
};

/** Reactive translation for components: `{t("home.record")}` */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  return translate(state.locale, key, params);
}
