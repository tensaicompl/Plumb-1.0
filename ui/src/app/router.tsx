import { createRootRoute, createRoute, createRouter, redirect, useParams, type RouterHistory } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { CalculationsPage } from "../features/calculations/CalculationsPage";
import { DecisionsPage } from "../features/decisions/DecisionsPage";
import { EntitiesPage } from "../features/entities/EntitiesPage";
import { FindingsPage } from "../features/findings/FindingsPage";
import { GatesPage } from "../features/gates/GatesPage";
import { GlossaryPage } from "../features/glossary/GlossaryPage";
import { OverviewScreen } from "../features/overview/OverviewScreen";
import { SourcesScreen } from "../features/sources/SourcesScreen";
import { ProcessesPage } from "../features/processes/ProcessesPage";
import { RequirementDetail } from "../features/requirements/RequirementDetail";
import { RequirementsPage } from "../features/requirements/RequirementsPage";
import { RulesPage } from "../features/rules/RulesPage";
import { ScenariosPage } from "../features/scenarios/ScenariosPage";
import { AppShell } from "./AppShell";
import { InboxScreen } from "../features/inbox/InboxScreen";
import { parseInboxSearch } from "../features/inbox/model";
import { Legacy, ModelInterim, QualityInterim } from "./routes/interim";
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
  component: OverviewScreen,
});

// ------------------------------------------------------------------ Inbox

// S2: the Inbox screen (src/features/inbox). One screen instance serves `/inbox` and
// `/inbox/$id` (the child routes only carry the selected item), so moving through the queue
// keeps its state. `?filter=` picks a smart filter.
const inboxRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/inbox",
  validateSearch: parseInboxSearch,
  component: function InboxRoute() {
    const { filter } = inboxRoute.useSearch();
    const params: { id?: string } = useParams({ strict: false });
    return <InboxScreen {...(filter === undefined ? {} : { filter })} {...(params.id === undefined ? {} : { selectedId: params.id })} />;
  },
});

const inboxIndexRoute = createRoute({ getParentRoute: () => inboxRoute, path: "/" });
const inboxItemRoute = createRoute({ getParentRoute: () => inboxRoute, path: "$id" });

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

const sourcesRoute = createRoute({ getParentRoute: () => rootRoute, path: "/sources", component: () => <SourcesScreen /> });

const sourceRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sources/$id",
  component: function SourceRoute() {
    const { id } = sourceRoute.useParams();
    return <SourcesScreen id={id} />;
  },
});

export const routeTree = rootRoute.addChildren([
  indexRoute,
  overviewRoute,
  inboxRoute.addChildren([inboxIndexRoute, inboxItemRoute]),
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
