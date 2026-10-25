import { useRouter, useRouterState, type ErrorComponentProps } from "@tanstack/react-router";
import type { MouseEvent } from "react";

import { t } from "../i18n";

/** A client-side link to Home. */
function HomeLink() {
  const router = useRouter();
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    router.history.push("/");
  };
  return (
    <a href="/" onClick={onClick} className="underline">
      {t("route.home")}
    </a>
  );
}

/** The typed not-found page of an unknown route, rendered inside the shell (Fable #352). */
export function NotFoundPage() {
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  return (
    <div className="space-y-2" data-testid="not-found">
      <h1 className="text-xl font-semibold">{t("route.notFound.title")}</h1>
      <p>
        {t("route.notFound.message")} <span className="font-mono" data-testid="not-found-path">{pathname}</span>
      </p>
      <HomeLink />
    </div>
  );
}

/** The catalogue-backed page of a route that failed to render (Fable #225). */
export function RouteErrorPage({ error }: ErrorComponentProps) {
  return (
    <div className="space-y-2" role="alert" data-testid="route-error">
      <h1 className="text-xl font-semibold">{t("route.error.title")}</h1>
      <p>{t("route.error.message")}</p>
      <p className="font-mono text-sm" data-testid="route-error-name">
        {error instanceof Error ? error.name : t("route.error.unknown")}
      </p>
      <HomeLink />
    </div>
  );
}
