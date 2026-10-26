import * as Dialog from "@radix-ui/react-dialog";
import { useQuery } from "@tanstack/react-query";
import {
  CircleHelp,
  Download,
  FileText,
  FlaskConical,
  LayoutGrid,
  List,
  MessageSquare,
  Moon,
  Play,
  Search,
  ShieldCheck,
  SlidersVertical,
  Sun,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

import { apiClient, readData } from "../../api/client";
import type { components } from "../../api/schema";
import { cn, Eyebrow, Kbd, modKey } from "../../components/ui";
import { t, tf, type MessageKey } from "../../i18n";
import { ALL_PAGE_SIZE, readAllPages } from "../../features/sources/queries";
import { DESTINATIONS } from "../nav";
import { RUNNABLE_STAGES, stageLabel, usePipeline } from "../pipeline";
import { isDarkAppearance, useAppearanceStore, useRecentStore, useShellStore, type RecentItem } from "../preferences";
import { useRunStage } from "../run";
import { buildHref, useGo } from "./navigation";

type SearchHit = components["schemas"]["SearchHit"];

/** The app page of a search hit, by its server kind; null when the kind has no page yet. */
export function hitHref(hit: Pick<SearchHit, "id" | "kind">): string | null {
  switch (hit.kind) {
    case "requirement":
      return buildHref("/requirements/$id", { id: hit.id });
    case "term":
    case "concept":
      return buildHref("/model/glossary/$id", { id: hit.id });
    case "entity":
      return buildHref("/model/entities/$id", { id: hit.id });
    case "rule":
      return buildHref("/model/rules/$id", { id: hit.id });
    case "calculation":
      return buildHref("/model/calculations/$id", { id: hit.id });
    case "process":
      return buildHref("/model/processes/$id", { id: hit.id });
    case "scenario":
      return buildHref("/scenarios/$id", { id: hit.id });
    case "finding":
      return "/quality/issues";
    case "question":
      return buildHref("/inbox", {}, { filter: "questions" });
    default:
      return null;
  }
}

const KIND_GROUP: Readonly<Record<string, { key: string; label: MessageKey; icon: LucideIcon }>> = {
  requirement: { key: "requirement", label: "palette.kind.requirement", icon: List },
  term: { key: "term", label: "palette.kind.term", icon: LayoutGrid },
  concept: { key: "term", label: "palette.kind.term", icon: LayoutGrid },
  entity: { key: "entity", label: "palette.kind.entity", icon: LayoutGrid },
  operation: { key: "operation", label: "palette.kind.operation", icon: LayoutGrid },
  rule: { key: "rule", label: "palette.kind.rule", icon: LayoutGrid },
  calculation: { key: "calculation", label: "palette.kind.calculation", icon: LayoutGrid },
  process: { key: "process", label: "palette.kind.process", icon: LayoutGrid },
  scenario: { key: "scenario", label: "palette.kind.scenario", icon: FlaskConical },
  finding: { key: "finding", label: "palette.kind.finding", icon: ShieldCheck },
  question: { key: "question", label: "palette.kind.question", icon: CircleHelp },
  evidence: { key: "evidence", label: "palette.kind.evidence", icon: FileText },
};

/** A requirement's document ID as a reader types it ("HR-009"). */
const DOCUMENT_ID = /^[A-Za-z][A-Za-z0-9]*-\d+$/;

function kindGroup(kind: string): { key: string; label: string; icon: LucideIcon } {
  const known = KIND_GROUP[kind];
  return known === undefined ? { key: kind, label: kind, icon: FileText } : { key: known.key, label: t(known.label), icon: known.icon };
}

/** One selectable row of the palette. */
interface PaletteOption {
  readonly id: string;
  readonly label: string;
  readonly sub?: string;
  readonly icon: ReactNode;
  readonly disabled?: boolean;
  readonly run: () => void;
}

interface PaletteGroup {
  readonly key: string;
  readonly label: string;
  readonly options: readonly PaletteOption[];
}

/** The debounce of server search while typing. */
export const SEARCH_DEBOUNCE_MS = 150;

const icon = (Icon: LucideIcon) => <Icon size={16} strokeWidth={1.6} aria-hidden="true" />;

function matches(option: { label: string; sub?: string }, query: string): boolean {
  return (option.label + " " + (option.sub ?? "")).toLowerCase().includes(query.toLowerCase());
}

function PaletteBody({ close }: { close: () => void }) {
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const go = useGo();
  const { run } = useRunStage();
  const pipeline = usePipeline();
  const appearance = useAppearanceStore((state) => state.appearance);
  const setAppearance = useAppearanceStore((state) => state.setAppearance);
  const openOverlay = useShellStore((state) => state.openOverlay);
  const setAssistantOpen = useShellStore((state) => state.setAssistantOpen);
  const setRunMenuOpen = useShellStore((state) => state.setRunMenuOpen);
  const recent = useRecentStore((state) => state.items);
  const remember = useRecentStore((state) => state.remember);

  const trimmed = query.trim();
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(trimmed), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [trimmed]);
  const search = useQuery({
    queryKey: ["search", debounced],
    queryFn: () => readData(apiClient.GET("/api/search", { params: { query: { q: debounced } } })),
    enabled: debounced !== "",
  });
  // Search indexes statements, not document IDs: a requirement ID ("HR-009") is matched against
  // the requirements' source identifiers.
  const byDocumentId = DOCUMENT_ID.test(debounced);
  const requirements = useQuery({
    queryKey: ["palette", "requirements"],
    queryFn: () => readAllPages((cursor) => apiClient.GET("/api/requirements", { params: { query: { limit: ALL_PAGE_SIZE, ...(cursor === undefined ? {} : { cursor }) } } })),
    enabled: byDocumentId,
  });

  const dark = isDarkAppearance(appearance);
  const goTo = (href: string) => () => {
    close();
    go(href);
  };
  const nextStage = pipeline.next;
  const allCommands: (PaletteOption & { readonly primary?: boolean })[] = [
    {
      id: "cmd-run-next",
      primary: true,
      label: t("palette.cmd.runNext"),
      sub: nextStage === null ? t("shell.run.menu") : stageLabel(nextStage),
      icon: icon(Play),
      run: () => {
        close();
        if (nextStage === null) {
          setRunMenuOpen(true);
        } else {
          run(nextStage);
        }
      },
    },
    ...DESTINATIONS.map((destination, index) => ({
      id: "cmd-go-" + destination.id,
      primary: destination.id === "inbox" || destination.id === "quality",
      label: tf("palette.cmd.go", { destination: t(destination.label) }),
      sub: modKey() + String(index + 1),
      icon: icon(destination.icon),
      run: goTo(destination.href),
    })),
    {
      id: "cmd-export",
      primary: true,
      label: t("palette.cmd.export"),
      sub: t("palette.cmd.exportHint"),
      icon: icon(Download),
      run: () => {
        close();
        openOverlay("export");
      },
    },
    {
      id: "cmd-appearance",
      primary: true,
      label: t(dark ? "palette.cmd.light" : "palette.cmd.dark"),
      icon: icon(dark ? Sun : Moon),
      run: () => {
        close();
        setAppearance(dark ? "light" : "dark");
      },
    },
    {
      id: "cmd-settings",
      label: t("palette.cmd.settings"),
      icon: icon(SlidersVertical),
      run: () => openOverlay("settings"),
    },
    {
      id: "cmd-assistant",
      label: t("palette.cmd.assistant"),
      icon: icon(MessageSquare),
      run: () => {
        close();
        setAssistantOpen(true);
      },
    },
    ...RUNNABLE_STAGES.map((stage) => ({
      id: "cmd-run-" + stage,
      label: tf("palette.cmd.runStep", { step: stageLabel(stage) }),
      icon: icon(Play),
      run: () => {
        close();
        run(stage);
      },
    })),
  ];

  const openHit = (item: RecentItem) => () => {
    remember(item);
    close();
    go(item.href);
  };

  const groups: PaletteGroup[] = [];
  const commands = trimmed === "" ? allCommands.filter((c) => c.primary === true) : allCommands.filter((c) => matches(c, trimmed));
  if (commands.length > 0) {
    groups.push({ key: "commands", label: t("palette.commands"), options: commands });
  }
  if (trimmed === "") {
    if (recent.length > 0) {
      groups.push({
        key: "recent",
        label: t("palette.recent"),
        options: recent.map((item) => ({ id: "recent-" + item.id, label: item.id, sub: item.snippet, icon: icon(kindGroup(item.kind).icon), run: openHit(item) })),
      });
    }
  } else if (debounced === trimmed && search.data !== undefined) {
    const identified = byDocumentId ? (requirements.data ?? []).filter((r) => r.source_identifier?.toLowerCase() === debounced.toLowerCase()) : [];
    if (identified.length > 0) {
      const group = kindGroup("requirement");
      groups.push({
        key: "kind-" + group.key,
        label: group.label,
        options: identified.map((r) => {
          const href = buildHref("/requirements/$id", { id: r.id });
          return { id: "hit-requirement-" + r.id, label: r.source_identifier ?? r.id, sub: r.text, icon: icon(group.icon), run: openHit({ id: r.source_identifier ?? r.id, kind: "requirement", snippet: r.text, href }) };
        }),
      });
    }
    for (const hit of search.data.items.filter((h) => !identified.some((r) => r.id === h.id))) {
      const group = kindGroup(hit.kind);
      const href = hitHref(hit);
      const option: PaletteOption = {
        id: "hit-" + hit.kind + "-" + hit.id,
        label: hit.id,
        sub: href === null ? t("palette.noPage") + " · " + hit.snippet : hit.snippet,
        icon: icon(group.icon),
        disabled: href === null,
        run: href === null ? () => undefined : openHit({ id: hit.id, kind: hit.kind, snippet: hit.snippet, href }),
      };
      const existing = groups.find((g) => g.key === "kind-" + group.key);
      if (existing === undefined) {
        groups.push({ key: "kind-" + group.key, label: group.label, options: [option] });
      } else {
        (existing.options as PaletteOption[]).push(option);
      }
    }
  }
  const flat = groups.flatMap((g) => g.options);
  const searching = trimmed !== "" && (debounced !== trimmed || search.isFetching || (byDocumentId && requirements.isFetching));
  const nothing = trimmed !== "" && !searching && flat.length === 0 && !search.isError;
  const activeIndex = Math.min(active, Math.max(0, flat.length - 1));
  const optionId = (index: number) => listId + "-option-" + String(index);
  const activeId = flat.length > 0 ? optionId(activeIndex) : null;
  useEffect(() => {
    if (activeId !== null) {
      document.getElementById(activeId)?.scrollIntoView({ block: "nearest" });
    }
  }, [activeId]);

  const execute = (option: PaletteOption | undefined) => {
    if (option !== undefined && option.disabled !== true) {
      option.run();
    }
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (flat.length > 0) {
        const step = event.key === "ArrowDown" ? 1 : -1;
        setActive((activeIndex + step + flat.length) % flat.length);
      }
    } else if (event.key === "Enter") {
      event.preventDefault();
      execute(flat[activeIndex]);
    }
  };

  let index = -1;
  return (
    <>
      <Dialog.Title className="sr-only">{t("palette.label")}</Dialog.Title>
      <Dialog.Description className="sr-only">{t("palette.placeholder")}</Dialog.Description>
      <div className="flex h-14 shrink-0 items-center gap-2.5 border-b border-line px-[18px] text-fg-3">
        <Search size={20} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
        <input
          ref={inputRef}
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-autocomplete="list"
          aria-label={t("palette.label")}
          aria-activedescendant={flat.length > 0 ? optionId(activeIndex) : undefined}
          value={query}
          placeholder={t("palette.placeholder")}
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
          className="min-w-0 flex-1 border-0 bg-transparent text-subhead text-fg outline-none placeholder:text-fg-3 focus-visible:shadow-none focus-visible:outline-none"
        />
        <Kbd>{t("palette.esc")}</Kbd>
      </div>
      {/* The results scroll by keyboard too (WCAG 2.1.1, axe scrollable-region-focusable); arrow keys
          in the input also move the active option into view. */}
      {/* eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- a scrollable region must be focusable */}
      <div tabIndex={0} role="region" aria-label={t("palette.results")} className="min-h-0 overflow-y-auto p-2">
        <div id={listId} role="listbox" aria-label={t("palette.label")}>
          {groups.map((group) => (
            <div key={group.key} role="group" aria-labelledby={listId + "-" + group.key}>
              <Eyebrow id={listId + "-" + group.key} className="px-2.5 pb-1.5 pt-2.5">
                {group.label}
              </Eyebrow>
              {group.options.map((option) => {
                index += 1;
                const position = index;
                const selected = position === activeIndex;
                return (
                  <div
                    key={option.id}
                    id={optionId(position)}
                    role="option"
                    aria-selected={selected}
                    aria-disabled={option.disabled === true ? true : undefined}
                    data-label={option.label}
                    // Keyboard focus stays in the combobox (aria-activedescendant); options are
                    // reachable with the arrow keys there and also respond to Enter if focused.
                    tabIndex={-1}
                    onMouseMove={() => setActive(position)}
                    onClick={() => execute(option)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        execute(option);
                      }
                    }}
                    className={cn(
                      "flex min-h-10 cursor-pointer items-center gap-3 rounded-control px-2.5 py-1.5",
                      selected && "bg-surface",
                      option.disabled === true && "cursor-default opacity-60",
                    )}
                  >
                    <span className="shrink-0 text-fg-3-surface">{option.icon}</span>
                    <span className="shrink-0 text-callout font-medium text-fg">{option.label}</span>
                    {option.sub !== undefined && <span className="min-w-0 flex-1 truncate text-caption text-fg-3-surface">{option.sub}</span>}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
        {searching && flat.length === 0 && <p className="m-0 px-2.5 py-6 text-center text-footnote text-fg-3">{t("palette.searching")}</p>}
        {search.isError && trimmed !== "" && <p className="m-0 px-2.5 py-3 text-center text-footnote text-fg-3">{t("palette.failed")}</p>}
        {nothing && (
          <p role="status" className="m-0 p-8 text-center text-footnote text-fg-3">
            {tf("palette.noResults", { q: trimmed })}
          </p>
        )}
        {trimmed !== "" && <p className="m-0 px-2.5 pb-1 pt-2 text-eyebrow text-fg-4">{t("shell.search.acceptedOnly")}</p>}
      </div>
    </>
  );
}

/** The ⌘K command palette: 620 px at 12vh, model search grouped by kind, commands and recent items. */
export function CommandPalette() {
  const open = useShellStore((state) => state.overlay === "palette");
  const openOverlay = useShellStore((state) => state.openOverlay);
  const closeOverlay = useShellStore((state) => state.closeOverlay);
  return (
    <Dialog.Root open={open} onOpenChange={(next) => (next ? openOverlay("palette") : closeOverlay())}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 flex animate-fade-in justify-center bg-scrim px-4 pt-[12vh]">
          <Dialog.Content className="flex max-h-[480px] w-[620px] max-w-full animate-overlay-in flex-col self-start overflow-hidden rounded-card bg-card text-fg shadow-xl outline-none">
            <PaletteBody close={closeOverlay} />
          </Dialog.Content>
        </Dialog.Overlay>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
