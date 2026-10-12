import type { components } from "../api/schema";
import { t, type MessageKey } from "../i18n";

export type ModelDiff = components["schemas"]["ModelDiff"];

const SECTIONS: readonly { key: "added" | "removed" | "modified"; label: MessageKey }[] = [
  { key: "added", label: "diff.added" },
  { key: "removed", label: "diff.removed" },
  { key: "modified", label: "diff.modified" },
];

/** The server ModelDiff: added, removed and modified element IDs, each in server order. */
export function DiffViewer({ diff }: { diff: ModelDiff }) {
  return (
    <section aria-label={t("diff.title")} className="space-y-3 text-sm">
      <dl className="grid grid-cols-[auto_1fr] gap-x-4">
        <dt>{t("diff.from")}</dt>
        <dd className="font-mono" data-testid="diff-from">
          {diff.from}
        </dd>
        <dt>{t("diff.to")}</dt>
        <dd className="font-mono" data-testid="diff-to">
          {diff.to}
        </dd>
      </dl>
      {SECTIONS.map(({ key, label }) => (
        <section key={key} aria-label={t(label)} data-diff={key}>
          <h3 className="font-semibold">
            {t(label)} <span data-testid="diff-count">{diff[key].length}</span>
          </h3>
          {diff[key].length === 0 ? (
            <p className="text-slate-500">{t("diff.none")}</p>
          ) : (
            <ul className="font-mono">
              {diff[key].map((id) => (
                <li key={id}>{id}</li>
              ))}
            </ul>
          )}
        </section>
      ))}
    </section>
  );
}
