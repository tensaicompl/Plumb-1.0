import type { ReactNode } from "react";

import { hasRole, useMe } from "../app/auth";
import { t } from "../i18n";

/** The role whose holders review and change the model (analyst write controls, Fable #218). */
export const ANALYST_ROLE = "analyst";

/**
 * Write controls that need `role` (per GET /api/me): rendered only when the server lists that
 * role for the current identity; otherwise an explanation names the missing role (or, `quiet`,
 * nothing, for a control repeated per row whose page explains it). While the identity loads
 * nothing is offered.
 */
export function RoleGate({ role = ANALYST_ROLE, quiet = false, children }: { role?: string; quiet?: boolean; children: ReactNode }) {
  const me = useMe();
  if (me.data === undefined) {
    return null;
  }
  if (!hasRole(me.data, role)) {
    if (quiet) {
      return null;
    }
    return (
      <p className="text-sm text-slate-600" data-testid="role-required">
        {t("role.required")} <span className="font-mono">{role}</span>
      </p>
    );
  }
  return <>{children}</>;
}
