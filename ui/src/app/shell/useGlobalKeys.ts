import { useEffect } from "react";

import { DESTINATIONS } from "../nav";
import { useShellStore } from "../preferences";
import { useGo } from "./navigation";

/**
 * The global shortcuts: ⌘K / Ctrl+K toggles the command palette; ⌘1–7 / Ctrl+1–7 jump to the seven
 * destinations (closing any shell overlay). Esc closes overlays and ⌘↩ confirms a sheet inside
 * the overlays themselves; screens add their own keys (Inbox J/K/A/R).
 */
export function useGlobalKeys(): void {
  const go = useGo();
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) {
        return;
      }
      const shell = useShellStore.getState();
      if (event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (shell.overlay === "palette") {
          shell.closeOverlay();
        } else {
          shell.openOverlay("palette");
        }
        return;
      }
      const destination = /^[1-7]$/.test(event.key) ? DESTINATIONS[Number(event.key) - 1] : undefined;
      if (destination !== undefined) {
        event.preventDefault();
        shell.closeOverlay();
        go(destination.href);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [go]);
}
