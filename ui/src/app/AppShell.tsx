import { useQuery } from "@tanstack/react-query";
import { Outlet, useRouter, useRouterState } from "@tanstack/react-router";
import { useState, type FormEvent, type MouseEvent } from "react";

import { apiClient } from "../api/client";
import type { components } from "../api/schema";
import { useEvents } from "../api/useEvents";
import { ConflictDialog } from "../components/ConflictDialog";
import { t } from "../i18n";
import { useMe } from "./auth";
import { PILOT_NAVIGATION, type NavLink } from "./nav";

type SearchPage = components["schemas"]["SearchPage"];

async function searchModel(q: string): Promise<SearchPage> {
  const { data, response } = await apiClient.GET("/api/search", { params: { query: { q } } });
  if (data === undefined) {
    throw new Error(`GET /api/search failed with HTTP ${response.status}`);
  }
  return data;
}

/** One navigation anchor; plain left clicks navigate client-side through the router history. */
function NavAnchor({ link, pathname }: { link: NavLink; pathname: string }) {
  const router = useRouter();
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    router.history.push(link.href);
  };
  return (
    <a
      href={link.href}
      onClick={onClick}
      aria-current={pathname === link.href ? "page" : undefined}
      data-nav-id={link.id}
      className="block rounded px-2 py-1 hover:bg-slate-100 aria-[current=page]:font-semibold"
    >
      {t(link.label)}
    </a>
  );
}

function Navigation() {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  return (
    <nav aria-label={t("nav.label")} className="w-56 shrink-0 border-r p-3">
      <ul className="space-y-1">
        {PILOT_NAVIGATION.map((entry) => {
          switch (entry.kind) {
            case "link":
              return (
                <li key={entry.link.id}>
                  <NavAnchor link={entry.link} pathname={pathname} />
                </li>
              );
            case "group":
              return (
                <li key={entry.id}>
                  <span className="block px-2 pt-2 text-xs font-semibold uppercase text-slate-500" data-nav-id={entry.id}>
                    {t(entry.label)}
                  </span>
                  <ul className="ml-2">
                    {entry.links.map((link) => (
                      <li key={link.id}>
                        <NavAnchor link={link} pathname={pathname} />
                      </li>
                    ))}
                  </ul>
                </li>
              );
            case "gates":
              return (
                <li key={entry.link.id}>
                  <NavAnchor link={entry.link} pathname={pathname} />
                  <ul className="ml-4 flex gap-2 text-xs font-mono" data-nav-id="gate-list">
                    {entry.gates.map((gate) => (
                      <li key={gate}>{gate}</li>
                    ))}
                  </ul>
                </li>
              );
          }
        })}
      </ul>
    </nav>
  );
}

function Identity() {
  const me = useMe();
  if (me.isError) {
    return <p role="alert">{t("shell.identityFailed")}</p>;
  }
  if (me.data === undefined) {
    return null;
  }
  return (
    <dl className="flex gap-4 text-sm" data-testid="identity">
      <div className="flex gap-1">
        <dt className="text-slate-500">{t("shell.user")}</dt>
        <dd data-testid="identity-user">{me.data.user}</dd>
      </div>
      <div className="flex gap-1">
        <dt className="text-slate-500">{t("shell.mode")}</dt>
        <dd data-testid="identity-mode">{me.data.mode}</dd>
      </div>
      <div className="flex gap-1">
        <dt className="text-slate-500">{t("shell.roles")}</dt>
        <dd>
          <ul className="flex gap-1" data-testid="identity-roles">
            {me.data.roles.map((role) => (
              <li key={role}>{role}</li>
            ))}
          </ul>
        </dd>
      </div>
    </dl>
  );
}

function GlobalSearch() {
  const [text, setText] = useState("");
  const [submitted, setSubmitted] = useState<string | null>(null);
  const results = useQuery({
    queryKey: ["search", submitted],
    queryFn: () => searchModel(submitted ?? ""),
    enabled: submitted !== null,
  });
  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (text.trim() !== "") {
      setSubmitted(text);
    }
  };
  return (
    <div className="relative">
      <form role="search" onSubmit={onSubmit} className="flex gap-2">
        <input
          type="search"
          value={text}
          onChange={(event) => setText(event.target.value)}
          aria-label={t("shell.search.label")}
          className="rounded border px-2 py-1"
        />
        <button type="submit" className="rounded border px-3 py-1">
          {t("shell.search.submit")}
        </button>
      </form>
      {submitted !== null && (
        <section aria-label={t("shell.search.results")} className="absolute right-0 z-10 mt-1 w-96 rounded border bg-white p-2 shadow">
          {results.isError && <p role="alert">{t("shell.search.failed")}</p>}
          {results.data !== undefined && results.data.items.length === 0 && <p>{t("shell.search.empty")}</p>}
          {results.data !== undefined && results.data.items.length > 0 && (
            <ol className="space-y-2" data-testid="search-results">
              {results.data.items.map((hit) => (
                <li key={hit.id} className="text-sm">
                  <span className="font-mono" data-testid="search-hit-id">
                    {hit.id}
                  </span>
                  <dl className="grid grid-cols-[auto_1fr] gap-x-2 text-xs">
                    <dt>{t("shell.search.kind")}</dt>
                    <dd>{hit.kind}</dd>
                    <dt>{t("shell.search.path")}</dt>
                    <dd className="font-mono">{hit.path}</dd>
                    <dt>{t("shell.search.score")}</dt>
                    <dd>{hit.score}</dd>
                  </dl>
                  <p className="text-xs">{hit.snippet}</p>
                </li>
              ))}
            </ol>
          )}
        </section>
      )}
    </div>
  );
}

/**
 * The application shell: identity header, global search, the pilot navigation, the DesignEvent
 * subscription, the single global ConflictDialog and the current route. It owns no model data.
 */
export function AppShell() {
  useEvents();
  return (
    <div className="flex min-h-screen flex-col">
      <header className="flex items-center justify-between gap-4 border-b px-4 py-2">
        <span className="text-lg font-semibold" data-testid="app-name">
          {t("app.name")}
        </span>
        <GlobalSearch />
        <Identity />
      </header>
      <div className="flex flex-1">
        <Navigation />
        <main className="flex-1 p-4">
          <Outlet />
        </main>
      </div>
      <ConflictDialog />
    </div>
  );
}
