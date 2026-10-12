import en from "./en.json";
import pl from "./pl.json";

/** The pilot UI locales. */
export type Locale = "en" | "pl";

/** Every message key, derived from the authoritative English catalogue. */
export type MessageKey = keyof typeof en;

type Catalogue = Readonly<Record<MessageKey, string>>;

// Catalogue parity is checked at compile time: Polish must define exactly the English keys.
type Exact<T, Shape> = T extends Shape ? (Exclude<keyof T, keyof Shape> extends never ? T : never) : never;
const polish: Exact<typeof pl, Catalogue> = pl;

const catalogues: Readonly<Record<Locale, Catalogue>> = { en, pl: polish };

/** The pilot default locale, also the fallback. */
export const DEFAULT_LOCALE: Locale = "en";

/** The message `key` in `locale`, falling back to English. */
export function t(key: MessageKey, locale: Locale = DEFAULT_LOCALE): string {
  return catalogues[locale][key] ?? catalogues[DEFAULT_LOCALE][key];
}
