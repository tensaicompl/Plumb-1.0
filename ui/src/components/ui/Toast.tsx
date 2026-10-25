import { Check, Info, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { create } from "zustand";

import { t } from "../../i18n";

/** How long a toast stays (the handoff: 3.8 s). Hovering or focusing it pauses the timer. */
export const TOAST_DURATION_MS = 3800;

export interface ToastInput {
  /** Sentence case, no exclamation mark ("Operation added to the model"). */
  readonly message: string;
  /** One optional action pill ("View in Model", "Review", "Undo"). */
  readonly action?: { readonly label: string; readonly onAction: () => void };
  readonly tone?: "success" | "info" | "error";
}

export interface ToastEntry extends ToastInput {
  readonly id: number;
  readonly tone: "success" | "info" | "error";
}

interface ToastState {
  readonly current: ToastEntry | null;
  show(input: ToastInput): void;
  dismiss(id?: number): void;
}

let nextId = 1;

/** View state of the single visible toast (a new toast replaces the current one). */
export const useToastStore = create<ToastState>()((set, get) => ({
  current: null,
  show: (input) => set({ current: { ...input, tone: input.tone ?? "success", id: nextId++ } }),
  dismiss: (id) => {
    if (id === undefined || get().current?.id === id) {
      set({ current: null });
    }
  },
}));

/** Shows a toast (callable from anywhere: handlers, mutation callbacks). */
export function toast(input: ToastInput): void {
  useToastStore.getState().show(input);
}

/** Dismisses the current toast (or only `id` when given). */
export function dismissToast(id?: number): void {
  useToastStore.getState().dismiss(id);
}

const ICON = {
  success: <Check size={16} strokeWidth={1.6} aria-hidden="true" className="text-toast-icon" />,
  info: <Info size={16} strokeWidth={1.6} aria-hidden="true" className="text-white/70" />,
  error: <TriangleAlert size={16} strokeWidth={1.6} aria-hidden="true" className="text-[#fca5a5]" />,
} as const;

function ToastCard({ entry }: { entry: ToastEntry }) {
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    if (paused) {
      return undefined;
    }
    const timer = setTimeout(() => dismissToast(entry.id), TOAST_DURATION_MS);
    return () => clearTimeout(timer);
  }, [entry.id, paused]);
  return (
    <div
      data-toast={entry.tone}
      role={entry.tone === "error" ? "alert" : "status"}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
      className="pointer-events-auto fixed bottom-7 left-1/2 z-[70] flex -translate-x-1/2 animate-fade-in items-center gap-3.5 rounded-pill bg-toast py-3 pl-[18px] pr-3.5 text-footnote font-medium text-white shadow-lg"
    >
      {ICON[entry.tone]}
      <span>{entry.message}</span>
      {entry.action !== undefined && (
        <button
          type="button"
          onClick={() => {
            dismissToast(entry.id);
            entry.action?.onAction();
          }}
          className="h-7 cursor-pointer rounded-pill bg-toast-action px-3 text-caption font-semibold text-white hover:brightness-125"
        >
          {entry.action.label}
        </button>
      )}
    </div>
  );
}

/** The toast region (mount once, in the shell): bottom-centre, 28 px from the bottom. */
export function Toaster() {
  const current = useToastStore((state) => state.current);
  return (
    <section aria-label={t("ui.toast.region")} className="pointer-events-none">
      {current !== null && <ToastCard key={current.id} entry={current} />}
    </section>
  );
}
