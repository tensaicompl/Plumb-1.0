import * as Checkbox from "@radix-ui/react-checkbox";
import { Check } from "lucide-react";
import { useId, useState } from "react";

import { apiClient, ReadFailedError } from "../../api/client";
import type { components } from "../../api/schema";
import { Sheet, toast } from "../../components/ui";
import { t, tc, tf, type MessageKey } from "../../i18n";
import { useShellStore } from "../preferences";

type ExportFormat = components["schemas"]["ExportFormat"];

/** The export formats, in checklist order, with their download file names. */
export const EXPORT_FORMATS: readonly { readonly format: ExportFormat; readonly label: MessageKey; readonly file: string }[] = [
  { format: "functional", label: "export.format.functional", file: "functional-model.yaml" },
  { format: "glossary", label: "export.format.glossary", file: "glossary.md" },
  { format: "scenarios", label: "export.format.scenarios", file: "scenarios.yaml" },
  { format: "report", label: "export.format.report", file: "report.md" },
  { format: "gate_report", label: "export.format.gate_report", file: "gate-report.json" },
  { format: "decisions", label: "export.format.decisions", file: "decisions.jsonl" },
];

/** Downloads one export (`export_model`) as a file. */
export async function downloadExport(format: ExportFormat, file: string): Promise<void> {
  const { data, error, response } = await apiClient.GET("/api/model/export", { params: { query: { format } }, parseAs: "blob" });
  if (!response.ok || data === undefined) {
    throw new ReadFailedError(response.status, error);
  }
  const url = URL.createObjectURL(data as Blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = file;
  anchor.click();
  URL.revokeObjectURL(url);
}

/** The Export sheet: a checklist of the six artifacts; each selected one downloads as its own file. */
export function ExportSheet() {
  const open = useShellStore((state) => state.overlay === "export");
  const closeOverlay = useShellStore((state) => state.closeOverlay);
  const [selected, setSelected] = useState<ReadonlySet<ExportFormat>>(() => new Set(EXPORT_FORMATS.map((f) => f.format)));
  const [pending, setPending] = useState(false);
  const baseId = useId();
  const download = async () => {
    setPending(true);
    const chosen = EXPORT_FORMATS.filter((f) => selected.has(f.format));
    try {
      for (const item of chosen) {
        await downloadExport(item.format, item.file);
      }
      closeOverlay();
      toast({ message: tc("export.done", chosen.length) });
    } catch (error) {
      const message = error instanceof ReadFailedError ? (error.serverMessage ?? error.code ?? String(error.status)) : t("read.transportFailed");
      toast({ tone: "error", message: tf("export.failed", { message }) });
    } finally {
      setPending(false);
    }
  };
  return (
    <Sheet
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          closeOverlay();
        }
      }}
      title={t("export.title")}
      description={t("export.description")}
      confirmLabel={t("export.download")}
      onConfirm={() => void download()}
      confirmDisabled={selected.size === 0}
      pending={pending}
    >
      <ul className="m-0 flex list-none flex-col overflow-hidden rounded-tile border border-line p-0">
        {EXPORT_FORMATS.map((item) => {
          const id = baseId + "-" + item.format;
          const checked = selected.has(item.format);
          return (
            <li key={item.format} className="flex items-center gap-2.5 border-b border-line px-3.5 py-[11px] text-footnote last:border-b-0">
              <Checkbox.Root
                id={id}
                checked={checked}
                onCheckedChange={(value) => {
                  const next = new Set(selected);
                  if (value === true) {
                    next.add(item.format);
                  } else {
                    next.delete(item.format);
                  }
                  setSelected(next);
                }}
                className="grid size-[18px] shrink-0 cursor-pointer place-items-center rounded-[5px] border border-line-strong bg-card text-on-primary data-[state=checked]:border-primary data-[state=checked]:bg-primary"
              >
                <Checkbox.Indicator>
                  <Check size={12} strokeWidth={2} aria-hidden="true" />
                </Checkbox.Indicator>
              </Checkbox.Root>
              <label htmlFor={id} className="flex-1 cursor-pointer font-medium text-fg">
                {t(item.label)}
              </label>
              <span className="font-mono text-caption text-fg-3">{item.file}</span>
            </li>
          );
        })}
      </ul>
    </Sheet>
  );
}
