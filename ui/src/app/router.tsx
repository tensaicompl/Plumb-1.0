import { createRootRoute, createRoute, createRouter, redirect, type RouterHistory } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { CalculationsPage } from "../features/calculations/CalculationsPage";
import { DecisionsPage } from "../features/decisions/DecisionsPage";
import { EntitiesPage } from "../features/entities/EntitiesPage";
import { FindingsPage } from "../features/findings/FindingsPage";
import { GatesPage } from "../features/gates/GatesPage";
import { GlossaryPage } from "../features/glossary/GlossaryPage";
import { HomePage } from "../features/home/HomePage";
import { ProcessesPage } from "../features/processes/ProcessesPage";
import { RequirementDetail } from "../features/requirements/RequirementDetail";
import { RequirementsPage } from "../features/requirements/RequirementsPage";
import { RulesPage } from "../features/rules/RulesPage";
import { ScenariosPage } from "../features/scenarios/ScenariosPage";
import { AppShell } from "./AppShell";
import { InboxInterim, Legacy, ModelInterim, parseInboxFilter, QualityInterim, SourceInterim, SourcesInterim } from "./routes/interim";
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

const requirementsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/requirements",
  component: () => (
    <Legacy>
      <RequirementsPage />
    </Legacy>
  ),
});

const requirementRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/requirements/$id",
  component: function RequirementRoute() {
    const { id } = requirementRoute.useParams();
    return (
      <Legacy>
        <RequirementDetail key={id} requirementId={id} />
      </Legacy>
    );
  },
});

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
  component: QualityInterim,
});

const qualityIndexRoute = createRoute({ getParentRoute: () => qualityRoute, path: "/", component: GatesPage });
const qualityGateRoute = createRoute({ getParentRoute: () => qualityRoute, path: "$gate", component: GatesPage });
const qualityHistoryRoute = createRoute({ getParentRoute: () => qualityRoute, path: "history", component: DecisionsPage });
const qualityIssuesRoute = createRoute({ getParentRoute: () => qualityRoute, path: "issues", component: FindingsPage });

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
  requirementsRoute,
  requirementRoute,
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
