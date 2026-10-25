import { createRootRoute, createRoute, createRouter, redirect, useParams, type RouterHistory } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { CalculationsPage } from "../features/calculations/CalculationsPage";
import { EntitiesPage } from "../features/entities/EntitiesPage";
import { GlossaryPage } from "../features/glossary/GlossaryPage";
import { HomePage } from "../features/home/HomePage";
import { ProcessesPage } from "../features/processes/ProcessesPage";
import { GatesTab } from "../features/quality/GatesTab";
import { HistoryTab } from "../features/quality/HistoryTab";
import { IssuesTab } from "../features/quality/IssuesTab";
import { QualityScreen } from "../features/quality/QualityScreen";
import { parseRequirementsSearch, RequirementsScreen } from "../features/requirements/RequirementsScreen";
import { RulesPage } from "../features/rules/RulesPage";
import { ScenariosPage } from "../features/scenarios/ScenariosPage";
import { AppShell } from "./AppShell";
import { InboxInterim, Legacy, ModelInterim, parseInboxFilter, SourceInterim, SourcesInterim } from "./routes/interim";
import { NotFoundPage, RouteErrorPage } from "./RouteStatus";

// The explicit route tree: the spec §3 URL structure, every URL deep-linkable. Screen lanes
// replace each route's component with their screen (src/README-SCREENS.md); until then a route
// renders the closest old page inside the new shell. There is no route discovery and no
// placeholder route. Unknown routes and render failures stay inside the shell.
const rootRoute = createRootRoute({
  component: AppShell,
  notFoundComponent: () => (
    <Legacy>
      <NotFoundPage />
    </Legacy>
  ),
  errorComponent: (props) => (
    <Legacy>
      <RouteErrorPage {...props} />
    </Legacy>
  ),
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/overview", replace: true });
  },
});

// ------------------------------------------------------------------ Overview

const overviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/overview",
  component: () => (
    <Legacy>
      <HomePage />
    </Legacy>
  ),
});

// ------------------------------------------------------------------ Inbox

const inboxRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inbox",
  validateSearch: parseInboxFilter,
  component: function InboxRoute() {
    const { filter } = inboxRoute.useSearch();
    return <InboxInterim filter={filter} />;
  },
});

const inboxItemRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inbox/$id",
  validateSearch: parseInboxFilter,
  component: function InboxItemRoute() {
    const { filter } = inboxItemRoute.useSearch();
    return <InboxInterim filter={filter} />;
  },
});

// ------------------------------------------------------------------ Requirements

// One screen for the list and the selected requirement: `$id` is a child route, so selecting a
// row keeps the screen (and its filters) mounted. `?affected=` comes from Quality → Show affected.
const requirementsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/requirements",
  validateSearch: parseRequirementsSearch,
  component: function RequirementsRoute() {
    const params = useParams({ strict: false }) as { id?: string };
    const { affected } = requirementsRoute.useSearch();
    return <RequirementsScreen selectedId={params.id} affected={affected === undefined ? undefined : affected.split(",").filter(Boolean)} />;
  },
});

const requirementRoute = createRoute({ getParentRoute: () => requirementsRoute, path: "$id", component: () => null });

// ------------------------------------------------------------------ Model

const modelRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model",
  component: ModelInterim,
});

const modelIndexRoute = createRoute({
  getParentRoute: () => modelRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/model/glossary", replace: true });
  },
});

/** A Model segment and its optional selected item (`/model/<segment>` and `/model/<segment>/$id`). */
function segmentRoutes(segment: "glossary" | "entities" | "rules" | "calculations" | "processes", Page: () => ReactNode) {
  return [
    createRoute({ getParentRoute: () => modelRoute, path: segment, component: Page }),
    createRoute({ getParentRoute: () => modelRoute, path: segment + "/$id", component: Page }),
  ] as const;
}

const glossaryRoutes = segmentRoutes("glossary", GlossaryPage);
const entitiesRoutes = segmentRoutes("entities", EntitiesPage);
const rulesRoutes = segmentRoutes("rules", RulesPage);
const calculationsRoutes = segmentRoutes("calculations", CalculationsPage);
const processesRoutes = segmentRoutes("processes", ProcessesPage);

// ------------------------------------------------------------------ Scenarios

const scenariosRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/scenarios",
  component: () => (
    <Legacy>
      <ScenariosPage />
    </Legacy>
  ),
});

const scenarioRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/scenarios/$id",
  component: () => (
    <Legacy>
      <ScenariosPage />
    </Legacy>
  ),
});

// ------------------------------------------------------------------ Quality

const qualityRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/quality",
  component: QualityScreen,
});

// /quality shows every gate collapsed; /quality/$gate expands that gate (Overview tiles link here).
const qualityIndexRoute = createRoute({ getParentRoute: () => qualityRoute, path: "/", component: () => <GatesTab expanded={null} /> });
const qualityGateRoute = createRoute({
  getParentRoute: () => qualityRoute,
  path: "$gate",
  component: function QualityGateRoute() {
    const { gate } = qualityGateRoute.useParams();
    return <GatesTab expanded={gate} />;
  },
});
const qualityHistoryRoute = createRoute({ getParentRoute: () => qualityRoute, path: "history", component: HistoryTab });
const qualityIssuesRoute = createRoute({ getParentRoute: () => qualityRoute, path: "issues", component: IssuesTab });

// ------------------------------------------------------------------ Sources

const sourcesRoute = createRoute({ getParentRoute: () => rootRoute, path: "/sources", component: SourcesInterim });

const sourceRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sources/$id",
  component: function SourceRoute() {
    const { id } = sourceRoute.useParams();
    return <SourceInterim key={id} id={id} />;
  },
});

export const routeTree = rootRoute.addChildren([
  indexRoute,
  overviewRoute,
  inboxRoute,
  inboxItemRoute,
  requirementsRoute.addChildren([requirementRoute]),
  modelRoute.addChildren([modelIndexRoute, ...glossaryRoutes, ...entitiesRoutes, ...rulesRoutes, ...calculationsRoutes, ...processesRoutes]),
  scenariosRoute,
  scenarioRoute,
  qualityRoute.addChildren([qualityIndexRoute, qualityGateRoute, qualityHistoryRoute, qualityIssuesRoute]),
  sourcesRoute,
  sourceRoute,
]);

/** A router over the route tree (browser history unless `history` is given). */
export function createAppRouter(history?: RouterHistory) {
  return createRouter({ routeTree, ...(history === undefined ? {} : { history }) });
}

export const router = createAppRouter();

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
