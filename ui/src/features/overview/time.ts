import { tf, type Locale } from "../../i18n";

/** A server timestamp (nanosecond precision allowed) as a Date. */
export function parseTimestamp(value: string): Date {
  // Trim fractional seconds to milliseconds: Date parsing is only specified for three digits.
  return new Date(value.replace(/(\.\d{3})\d+/, "$1"));
}

const UNITS: readonly [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
];

/** "4 minutes ago", "yesterday", "just now" in `locale`. */
export function relativeTime(value: string, now: Date, locale: Locale): string {
  const seconds = Math.round((parseTimestamp(value).getTime() - now.getTime()) / 1000);
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) {
      return format.format(Math.round(seconds / size), unit);
    }
  }
  return format.format(0, "second");
}

function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/** "Today, 10:42", "Yesterday, 16:30" or "21 Oct, 15:48" (local time). */
export function decisionTime(value: string, now: Date, locale: Locale): string {
  const date = parseTimestamp(value);
  const time = date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hour12: false });
  if (sameDay(date, now)) {
    return tf("overview.time.today", { time }, locale);
  }
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (sameDay(date, yesterday)) {
    return tf("overview.time.yesterday", { time }, locale);
  }
  return tf("overview.time.date", { date: date.toLocaleDateString(locale, { day: "numeric", month: "short" }), time }, locale);
}

/** The readable name of an actor reference ("agent:marta-nowak" → "marta-nowak"). */
export function actorName(actor: string): string {
  const colon = actor.indexOf(":");
  return colon === -1 ? actor : actor.slice(colon + 1);
}
