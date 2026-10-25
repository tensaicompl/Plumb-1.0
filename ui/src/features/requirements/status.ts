import { PILL_STATUSES, type PillStatus } from "../../components/ui";
import { t, type MessageKey } from "../../i18n";

/** The StatusPill status of a wire lifecycle status ("Accepted" → accepted), or null when unknown. */
export function pillStatus(status: string): PillStatus | null {
  const key = status.toLowerCase();
  return (PILL_STATUSES as readonly string[]).includes(key) ? (key as PillStatus) : null;
}

/** The catalogue label of a wire lifecycle status; an unknown status shows verbatim. */
export function statusLabel(status: string): string {
  const pill = pillStatus(status);
  return pill === null ? status : t(("ui.status." + pill) as MessageKey);
}
