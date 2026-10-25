// The padded scrolling column the shell's not-found and route-error pages render in. Every route
// now renders its own screen (src/README-SCREENS.md).
import type { ReactNode } from "react";

/** A plain page inside the shell: a scrolling column with the page padding. */
export function Legacy({ children }: { children: ReactNode }) {
  return <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-16 pt-8">{children}</div>;
}
