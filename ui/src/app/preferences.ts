import { useSyncExternalStore } from "react";
import { create } from "zustand";

// Per-browser view preferences and shell view state. Nothing here is server data; storage may be
// unavailable (privacy settings, previews), so every read and write is guarded.

function readStorage(key: string): string | null {
  try {
    return globalThis.localStorage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string): void {
  try {
    globalThis.localStorage?.setItem(key, value);
  } catch {
    // Not persisted when storage is unavailable; the choice still applies to this session.
  }
}

// ------------------------------------------------------------------ appearance

export type Appearance = "system" | "light" | "dark";

export const APPEARANCES: readonly Appearance[] = ["system", "light", "dark"];

/** The browser storage key of the appearance choice. */
export const APPEARANCE_STORAGE_KEY = "plumb.appearance";

function isAppearance(value: unknown): value is Appearance {
  return value === "system" || value === "light" || value === "dark";
}

/** The stored appearance, else System. */
export function initialAppearance(): Appearance {
  const stored = readStorage(APPEARANCE_STORAGE_KEY);
  return isAppearance(stored) ? stored : "system";
}

/** Applies an appearance to the document: `data-appearance` on <html> (absent for System). */
export function applyAppearance(appearance: Appearance, root: HTMLElement = document.documentElement): void {
  if (appearance === "system") {
    delete root.dataset["appearance"];
  } else {
    root.dataset["appearance"] = appearance;
  }
}

/** Whether the effective appearance is dark (System follows the OS preference). */
export function isDarkAppearance(appearance: Appearance): boolean {
  if (appearance !== "system") {
    return appearance === "dark";
  }
  return globalThis.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

interface AppearanceState {
  readonly appearance: Appearance;
  setAppearance(appearance: Appearance): void;
}

/** The appearance choice (Settings), persisted per browser and applied immediately. */
export const useAppearanceStore = create<AppearanceState>()((set) => ({
  appearance: initialAppearance(),
  setAppearance: (appearance) => {
    writeStorage(APPEARANCE_STORAGE_KEY, appearance);
    applyAppearance(appearance);
    set({ appearance });
  },
}));

// ------------------------------------------------------------------ shell view state

/** The browser storage key of the collapsed sidebar. */
export const SIDEBAR_STORAGE_KEY = "plumb.sidebar";

type Overlay = "palette" | "settings" | "export" | null;

interface ShellState {
  readonly sidebarCollapsed: boolean;
  readonly assistantOpen: boolean;
  /** The one open shell overlay. */
  readonly overlay: Overlay;
  /** The Run step menu (opened from the toolbar or the palette's "Run next step"). */
  readonly runMenuOpen: boolean;
  toggleSidebar(): void;
  setAssistantOpen(open: boolean): void;
  openOverlay(overlay: Exclude<Overlay, null>): void;
  closeOverlay(): void;
  setRunMenuOpen(open: boolean): void;
}

/** Shell view state: sidebar, Assistant panel and the palette / settings / export overlays. */
export const useShellStore = create<ShellState>()((set, get) => ({
  sidebarCollapsed: readStorage(SIDEBAR_STORAGE_KEY) === "collapsed",
  assistantOpen: false,
  overlay: null,
  runMenuOpen: false,
  setRunMenuOpen: (runMenuOpen) => set({ runMenuOpen }),
  toggleSidebar: () => {
    const collapsed = !get().sidebarCollapsed;
    writeStorage(SIDEBAR_STORAGE_KEY, collapsed ? "collapsed" : "expanded");
    set({ sidebarCollapsed: collapsed });
  },
  setAssistantOpen: (assistantOpen) => set({ assistantOpen }),
  openOverlay: (overlay) => set({ overlay }),
  closeOverlay: () => set({ overlay: null }),
}));

// ------------------------------------------------------------------ recent palette items

/** A search hit the user opened from the palette. */
export interface RecentItem {
  readonly id: string;
  readonly kind: string;
  readonly snippet: string;
  readonly href: string;
}

/** The browser storage key of the palette's recent items. */
export const RECENT_STORAGE_KEY = "plumb.recent";

/** At most this many recent items are kept. */
export const RECENT_LIMIT = 5;

function initialRecent(): RecentItem[] {
  try {
    const value: unknown = JSON.parse(readStorage(RECENT_STORAGE_KEY) ?? "[]");
    return Array.isArray(value)
      ? value.filter((v): v is RecentItem => typeof v === "object" && v !== null && typeof v.id === "string" && typeof v.href === "string").slice(0, RECENT_LIMIT)
      : [];
  } catch {
    return [];
  }
}

interface RecentState {
  readonly items: readonly RecentItem[];
  remember(item: RecentItem): void;
}

/** The palette's recently opened items (most recent first), persisted per browser. */
export const useRecentStore = create<RecentState>()((set, get) => ({
  items: initialRecent(),
  remember: (item) => {
    const items = [item, ...get().items.filter((existing) => existing.id !== item.id)].slice(0, RECENT_LIMIT);
    writeStorage(RECENT_STORAGE_KEY, JSON.stringify(items));
    set({ items });
  },
}));

// ------------------------------------------------------------------ viewport

/** Below this width the toolbar is compact (icon-only search and run, short version pill). */
export const COMPACT_QUERY = "(max-width: 1279px)";

function subscribeCompact(onChange: () => void): () => void {
  const list = globalThis.matchMedia?.(COMPACT_QUERY);
  list?.addEventListener("change", onChange);
  return () => list?.removeEventListener("change", onChange);
}

function compactSnapshot(): boolean {
  return globalThis.matchMedia?.(COMPACT_QUERY).matches ?? false;
}

/** Whether the viewport is narrower than 1280 px (the compact toolbar layout). */
export function useIsCompact(): boolean {
  return useSyncExternalStore(subscribeCompact, compactSnapshot, () => false);
}
