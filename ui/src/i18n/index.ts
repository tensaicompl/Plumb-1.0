import { create } from "zustand";

import en from "./en.json";
import pl from "./pl.json";

/** The pilot UI locales. */
export type Locale = "en" | "pl";

/** The supported locales, in selection order. */
export const LOCALES: readonly Locale[] = ["en", "pl"];

/** Every message key, derived from the authoritative English catalogue. */
export type MessageKey = keyof typeof en;

type Catalogue = Readonly<Record<MessageKey, string>>;

// Catalogue parity is checked at compile time: Polish must define exactly the English keys.
type Exact<T, Shape> = T extends Shape ? (Exclude<keyof T, keyof Shape> extends never ? T : never) : never;
const polish: Exact<typeof pl, Catalogue> = pl;

const catalogues: Readonly<Record<Locale, Catalogue>> = { en, pl: polish };

/** The pilot default locale, also the fallback. */
export const DEFAULT_LOCALE: Locale = "en";

/** The browser storage key of the selected locale (per browser, never sent to the server). */
export const LOCALE_STORAGE_KEY = "plumb.locale";

function isLocale(value: unknown): value is Locale {
  return value === "en" || value === "pl";
}

/** The stored selection, else the browser language when supported, else English. */
export function initialLocale(): Locale {
  try {
    const stored = globalThis.localStorage?.getItem(LOCALE_STORAGE_KEY);
    if (isLocale(stored)) {
      return stored;
    }
  } catch {
    // Storage may be unavailable (privacy settings); fall through to the browser language.
  }
  const language = globalThis.navigator?.language?.toLowerCase() ?? "";
  return language === "pl" || language.startsWith("pl-") ? "pl" : DEFAULT_LOCALE;
}

interface LocaleState {
  readonly locale: Locale;
  setLocale(locale: Locale): void;
}

/** The selected UI locale (view state only); a change is persisted for this browser. */
export const useLocaleStore = create<LocaleState>()((set) => ({
  locale: initialLocale(),
  setLocale: (locale) => {
    try {
      globalThis.localStorage?.setItem(LOCALE_STORAGE_KEY, locale);
    } catch {
      // Not persisted when storage is unavailable; the selection still applies.
    }
    set({ locale });
  },
}));

/** The message `key` in `locale` (the selected locale by default), falling back to English. */
export function t(key: MessageKey, locale: Locale = useLocaleStore.getState().locale): string {
  return catalogues[locale][key] ?? catalogues[DEFAULT_LOCALE][key];
}
