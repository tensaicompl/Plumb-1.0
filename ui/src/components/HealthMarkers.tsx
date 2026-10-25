import type { components } from "../api/schema";
import { t } from "../i18n";

export type ItemHealth = components["schemas"]["ItemHealth"];

/**
 * The items of a list the server could not project (H0.11): each element ID with its typed error
 * code and message. Corrupt items are reported, never hidden; the rest of the list is unaffected.
 */
export function HealthMarkers({ health }: { health: readonly ItemHealth[] | undefined }) {
  if (health === undefined || health.length === 0) {
    return null;
  }
  return (
    <section aria-label={t("health.title")} className="space-y-1 rounded border border-red-700 p-2 text-sm" data-testid="health-markers">
      <h2 className="font-semibold text-red-800">{t("health.title")}</h2>
      <p>{t("health.note")}</p>
      <ul>
        {health.map((marker) => (
          <li key={marker.id} data-health-id={marker.id}>
            <span className="font-mono">{marker.id}</span> <span className="font-mono" data-testid="health-code">{marker.code}</span> <span>{marker.message}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
