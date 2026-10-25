import { useQuery } from "@tanstack/react-query";

import { apiClient, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { Sheet, SkeletonList, Tooltip } from "../../components/ui";
import { t, tf, useLocaleStore } from "../../i18n";
import { useLabels } from "./labels";
import { ReadError } from "./shared";

type CrudCell = components["schemas"]["CrudCell"];

export const crudQuery = {
  queryKey: ["model", "crud"] as const,
  queryFn: () => readData(apiClient.GET("/api/model/crud")),
};

const LETTERS = ["c", "r", "u", "d"] as const;
type Letter = (typeof LETTERS)[number];

const VERB = { c: "model.coverage.verb.c", r: "model.coverage.verb.r", u: "model.coverage.verb.u", d: "model.coverage.verb.d" } as const;
const NOUN = { c: "model.coverage.noun.c", r: "model.coverage.noun.r", u: "model.coverage.noun.u", d: "model.coverage.noun.d" } as const;

/** "reads and updates" / "odczytuje i aktualizuje". */
export function joinList(items: readonly string[], locale: string): string {
  return new Intl.ListFormat(locale, { type: "conjunction" }).format(items);
}

/** The plain sentence explaining one CRUD cell; `null` flags are "not distinguishable", never "no". */
export function describeCell(cell: CrudCell | undefined, op: string, ent: string, locale: string): string {
  const on = LETTERS.filter((letter) => cell?.[letter] === true);
  const unknown = LETTERS.filter((letter) => cell !== undefined && cell[letter] === null);
  let sentence =
    on.length === 0
      ? tf("model.coverage.cell.none", { op, ent })
      : tf("model.coverage.cell.access", { op, ent, verbs: joinList(on.map((l) => t(VERB[l])), locale) });
  if (unknown.length > 0) {
    sentence += tf("model.coverage.cell.unknown", { flags: joinList(unknown.map((l) => t(NOUN[l])), locale) });
  }
  return sentence;
}

/** The CRUD matrix (get_crud_matrix): operations × entities; hovering or focusing a cell explains it. */
export function CoverageSheet({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const crud = useQuery({ ...crudQuery, enabled: open });
  const locale = useLocaleStore((state) => state.locale);
  const matrix = crud.data;
  const label = useLabels(matrix === undefined ? [] : [...matrix.operations, ...matrix.entities]);
  const cellOf = (op: string, ent: string) => matrix?.cells.find((cell) => cell.op === op && cell.ent === ent);
  return (
    <Sheet open={open} onOpenChange={onOpenChange} title={t("model.coverage.title")} description={t("model.coverage.description")} width={760} cancelLabel={t("model.close")}>
      {crud.isError && <ReadError error={crud.error} messageKey="model.coverage.loadFailed" onRetry={() => void crud.refetch()} />}
      {(crud.isPending || (matrix !== undefined && !label.ready)) && !crud.isError && <SkeletonList rows={4} />}
      {matrix !== undefined && label.ready && (matrix.operations.length === 0 || matrix.entities.length === 0) && (
        <p className="m-0 text-callout text-fg-3">{t("model.coverage.empty")}</p>
      )}
      {matrix !== undefined && label.ready && matrix.operations.length > 0 && matrix.entities.length > 0 && (
        <div className="max-h-[60vh] overflow-auto rounded-tile border border-line">
          <table className="w-full border-collapse text-caption">
            <caption className="sr-only">{t("model.coverage.caption")}</caption>
            <thead>
              <tr className="bg-surface">
                <th scope="col" className="w-[200px] border-b border-line px-2.5 py-[9px] text-left text-eyebrow font-semibold text-fg-3-surface">
                  {t("model.coverage.operation")}
                </th>
                {matrix.entities.map((ent) => (
                  <th key={ent} scope="col" className="border-b border-line px-2.5 py-[9px] text-center text-eyebrow font-semibold text-fg-3-surface">
                    {label(ent)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {matrix.operations.map((op) => (
                <tr key={op}>
                  <th scope="row" className="border-b border-line px-2.5 py-[9px] text-left font-medium text-fg">
                    {label(op)}
                  </th>
                  {matrix.entities.map((ent) => {
                    const cell = cellOf(op, ent);
                    const letters = LETTERS.filter((letter: Letter) => cell?.[letter] === true)
                      .map((letter) => letter.toUpperCase())
                      .join("");
                    const sentence = describeCell(cell, label(op), label(ent), locale);
                    return (
                      <td key={ent} aria-label={sentence} className="border-b border-line px-2.5 py-[9px] text-center">
                        <Tooltip content={sentence}>
                          {/* Focusable so keyboard users get the same explanation as on hover. */}
                          {/* eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex */}
                          <span tabIndex={0} className={letters === "" ? "font-sans text-fg-3" : "font-mono font-semibold text-fg"}>
                            {letters === "" ? "·" : letters}
                          </span>
                        </Tooltip>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Sheet>
  );
}
