import * as Dialog from "@radix-ui/react-dialog";

import { t, type MessageKey } from "../i18n";

/**
 * One provenance item. The caller states the category; it is never inferred from a reference
 * string. `ref` is the technical reference (fragment, derivation, decision or artifact identity).
 */
export type ProvenanceItem =
  | { readonly category: "evidence"; readonly ref: string; readonly detail?: string }
  | { readonly category: "deterministic_derivation"; readonly ref: string; readonly detail?: string }
  | { readonly category: "human_resolution"; readonly ref: string; readonly detail?: string }
  | { readonly category: "llm_inference"; readonly ref: string; readonly detail?: string };

export type ProvenanceCategory = ProvenanceItem["category"];

export const PROVENANCE_CATEGORIES: readonly ProvenanceCategory[] = [
  "evidence",
  "deterministic_derivation",
  "human_resolution",
  "llm_inference",
];

const LABEL: Readonly<Record<ProvenanceCategory, MessageKey>> = {
  evidence: "provenance.evidence",
  deterministic_derivation: "provenance.deterministicDerivation",
  human_resolution: "provenance.humanResolution",
  llm_inference: "provenance.llmInference",
};

const STYLE: Readonly<Record<ProvenanceCategory, string>> = {
  evidence: "border-l-4 border-sky-600",
  deterministic_derivation: "border-l-4 border-emerald-600",
  human_resolution: "border-l-4 border-amber-600",
  llm_inference: "border-l-4 border-fuchsia-600",
};

/** A side drawer listing the provenance of `subjectRef`, grouped by its four distinct classes. */
export function ProvenanceDrawer({
  open,
  onOpenChange,
  subjectRef,
  items,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  subjectRef: string;
  items: readonly ProvenanceItem[];
}) {
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/30" />
        <Dialog.Content className="fixed inset-y-0 right-0 w-full max-w-md overflow-y-auto bg-white p-6 shadow-lg">
          <Dialog.Title className="text-lg font-semibold">{t("provenance.title")}</Dialog.Title>
          <Dialog.Description className="font-mono text-sm" data-testid="provenance-subject">
            {subjectRef}
          </Dialog.Description>
          {items.length === 0 && <p className="mt-4 text-sm">{t("provenance.empty")}</p>}
          {PROVENANCE_CATEGORIES.map((category) => {
            const inCategory = items.filter((item) => item.category === category);
            if (inCategory.length === 0) {
              return null;
            }
            return (
              <section key={category} aria-label={t(LABEL[category])} data-category={category} className={"mt-4 pl-3 " + STYLE[category]}>
                <h3 className="font-semibold">{t(LABEL[category])}</h3>
                <ul className="text-sm">
                  {inCategory.map((item) => (
                    <li key={item.ref} data-category={item.category}>
                      <span className="font-mono">{item.ref}</span>
                      {item.detail !== undefined && <span className="block text-slate-600">{item.detail}</span>}
                    </li>
                  ))}
                </ul>
              </section>
            );
          })}
          <Dialog.Close asChild>
            <button type="button" className="mt-6 rounded border px-3 py-1">
              {t("provenance.close")}
            </button>
          </Dialog.Close>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
