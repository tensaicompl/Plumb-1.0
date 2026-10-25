/** Joins class names, skipping falsy parts. */
export function cn(...parts: readonly (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

/** Whether a click is a plain primary click (no modifier): such clicks navigate client-side. */
export function isPlainClick(event: { button: number; metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean }): boolean {
  return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;
}

/** Whether the platform uses ⌘ (Apple) rather than Ctrl for shortcuts. */
export function isApplePlatform(): boolean {
  const platform = globalThis.navigator?.platform ?? "";
  const agent = globalThis.navigator?.userAgent ?? "";
  return /Mac|iPhone|iPad/.test(platform) || /Mac OS X/.test(agent);
}

/** The shortcut modifier symbol: ⌘ on Apple platforms, Ctrl elsewhere. */
export function modKey(): string {
  return isApplePlatform() ? "⌘" : "Ctrl";
}

/** The label of the confirm shortcut (⌘↩ / Ctrl↩). */
export function confirmShortcut(): string {
  return modKey() + "↩";
}
