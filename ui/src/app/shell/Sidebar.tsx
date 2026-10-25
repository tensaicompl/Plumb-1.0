import { useRouterState } from "@tanstack/react-router";
import { MessageSquare, PanelLeftClose, SlidersVertical } from "lucide-react";
import type { MouseEvent, ReactNode } from "react";

import { Avatar, cn, EnsoMark, isPlainClick } from "../../components/ui";
import { t, tc, type MessageKey } from "../../i18n";
import { useMe } from "../auth";
import { DESTINATIONS, navCurrent, type Destination } from "../nav";
import { useShellStore } from "../preferences";
import { useWorkCounts } from "../work";
import { useGo } from "./navigation";

const ROLE_LABEL: Readonly<Record<string, MessageKey>> = {
  analyst: "shell.role.analyst",
  reviewer: "shell.role.reviewer",
  architect: "shell.role.architect",
  business: "shell.role.business",
  admin: "shell.role.admin",
};

/** The display of the server's roles ("Analyst · Reviewer"); unknown roles show verbatim. */
export function rolesLabel(roles: readonly string[]): string {
  return roles.map((role) => (role in ROLE_LABEL ? t(ROLE_LABEL[role] as MessageKey) : role)).join(" · ");
}

/** The badge text: the count, "99+" above 99. */
export function badgeText(count: number): string {
  return count > 99 ? "99+" : String(count);
}

const ITEM =
  "flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-control px-2.5 text-left text-callout leading-none no-underline transition-colors duration-[120ms] ease-standard";

function NavItem({ destination, pathname, collapsed, badge }: { destination: Destination; pathname: string; collapsed: boolean; badge: number | undefined }) {
  const go = useGo();
  const current = navCurrent(destination, pathname);
  const active = current !== undefined;
  const Icon = destination.icon;
  const label = t(destination.label);
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    if (isPlainClick(event)) {
      event.preventDefault();
      go(destination.href);
    }
  };
  const showBadge = badge !== undefined && badge > 0;
  const countText = badge === undefined ? null : tc("shell.inbox.badge", badge);
  return (
    <a
      href={destination.href}
      onClick={onClick}
      aria-current={current}
      aria-label={collapsed ? (countText === null ? label : label + ", " + countText) : undefined}
      title={collapsed ? label : undefined}
      data-nav-id={destination.id}
      className={cn(ITEM, "relative", active ? "bg-primary font-semibold text-on-primary" : "font-medium text-fg-2 hover:bg-surface")}
    >
      <Icon size={18} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />
      {!collapsed && <span className="min-w-0 flex-1 truncate">{label}</span>}
      {showBadge && (
        <span
          aria-hidden="true"
          data-badge
          className={cn(
            "rounded-pill text-center font-mono font-semibold",
            collapsed ? "absolute right-1 top-1 h-4 min-w-4 px-1 text-[9px] leading-4" : "h-5 min-w-[22px] px-1.5 text-eyebrow leading-5",
            active ? "bg-primary-soft text-on-primary" : "bg-surface text-fg",
          )}
        >
          {badgeText(badge)}
        </span>
      )}
      {countText !== null && !collapsed && <span className="sr-only">{", " + countText}</span>}
    </a>
  );
}

function SideButton({ icon, label, onClick, collapsed, pressed }: { icon: ReactNode; label: string; onClick: () => void; collapsed: boolean; pressed?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={collapsed ? label : undefined}
      aria-pressed={pressed}
      title={collapsed ? label : undefined}
      className={cn(ITEM, "font-medium text-fg-2 hover:bg-surface", pressed === true && "bg-surface")}
    >
      {icon}
      {!collapsed && <span>{label}</span>}
    </button>
  );
}

function Identity({ collapsed }: { collapsed: boolean }) {
  const me = useMe();
  if (me.data === undefined) {
    return null;
  }
  return (
    <div className="mt-auto flex items-center gap-2.5 border-t border-line px-4 py-3.5" title={collapsed ? me.data.user : undefined}>
      <Avatar name={me.data.user} decorative={!collapsed} />
      {!collapsed && (
        <div className="min-w-0">
          <div className="truncate text-footnote font-semibold text-fg">{me.data.user}</div>
          <div className="truncate text-eyebrow text-fg-3">{rolesLabel(me.data.roles)}</div>
        </div>
      )}
    </div>
  );
}

/** The sidebar: 232 px (64 collapsed), the seven destinations, Assistant, Settings and the user. */
export function Sidebar() {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const collapsed = useShellStore((state) => state.sidebarCollapsed);
  const toggle = useShellStore((state) => state.toggleSidebar);
  const assistantOpen = useShellStore((state) => state.assistantOpen);
  const setAssistantOpen = useShellStore((state) => state.setAssistantOpen);
  const openOverlay = useShellStore((state) => state.openOverlay);
  const work = useWorkCounts();
  return (
    <aside
      style={{ width: collapsed ? 64 : 232 }}
      className="flex shrink-0 flex-col overflow-hidden border-r border-line bg-sidebar transition-[width] duration-200 ease-standard"
    >
      <div className="group flex h-14 shrink-0 items-center gap-1 border-b border-line px-3">
        {collapsed ? (
          <button type="button" onClick={toggle} aria-label={t("shell.sidebar.expand")} title={t("shell.sidebar.expand")} className="cursor-pointer rounded-control">
            <EnsoMark size={40} />
          </button>
        ) : (
          <>
            <EnsoMark size={40} />
            <span className="font-display text-title-sm font-extrabold tracking-display text-fg">{t("app.name")}</span>
            <button
              type="button"
              onClick={toggle}
              aria-label={t("shell.sidebar.collapse")}
              title={t("shell.sidebar.collapse")}
              className="ml-auto grid size-[30px] cursor-pointer place-items-center rounded-control text-fg-3 opacity-0 transition-opacity duration-[120ms] hover:bg-surface focus-visible:opacity-100 group-hover:opacity-100"
            >
              <PanelLeftClose size={16} strokeWidth={1.6} aria-hidden="true" />
            </button>
          </>
        )}
      </div>
      <nav aria-label={t("nav.label")} className="flex flex-col gap-0.5 px-2 py-3">
        {DESTINATIONS.map((destination) => (
          <NavItem key={destination.id} destination={destination} pathname={pathname} collapsed={collapsed} badge={destination.id === "inbox" ? work.inbox : undefined} />
        ))}
      </nav>
      <div aria-hidden="true" className="mx-4 my-1 h-px shrink-0 bg-line" />
      <div className="flex flex-col gap-0.5 p-2">
        <SideButton
          icon={<MessageSquare size={18} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />}
          label={t("nav.assistant")}
          collapsed={collapsed}
          pressed={assistantOpen}
          onClick={() => setAssistantOpen(!assistantOpen)}
        />
        <SideButton
          icon={<SlidersVertical size={18} strokeWidth={1.6} aria-hidden="true" className="shrink-0" />}
          label={t("nav.settings")}
          collapsed={collapsed}
          onClick={() => openOverlay("settings")}
        />
      </div>
      <Identity collapsed={collapsed} />
    </aside>
  );
}
