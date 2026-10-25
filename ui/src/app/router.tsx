import { createRootRoute, createRoute, createRouter, redirect, useParams, type RouterHistory } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { OverviewScreen } from "../features/overview/OverviewScreen";
import { SourcesScreen } from "../features/sources/SourcesScreen";
import { GlossaryView } from "../features/model/GlossaryView";
import { ModelScreen } from "../features/model/ModelScreen";
import { EntitiesView } from "../features/model/EntitiesView";
import { RulesView } from "../features/model/RulesView";
import { CalculationsView } from "../features/model/CalculationsView";
import { ProcessesView } from "../features/model/ProcessesView";
import { GatesTab } from "../features/quality/GatesTab";
import { HistoryTab } from "../features/quality/HistoryTab";
import { IssuesTab } from "../features/quality/IssuesTab";
import { QualityScreen } from "../features/quality/QualityScreen";
import { parseRequirementsSearch, RequirementsScreen } from "../features/requirements/RequirementsScreen";
import { ScenariosScreen } from "../features/scenarios/ScenariosScreen";
import { AppShell } from "./AppShell";
import { InboxScreen } from "../features/inbox/InboxScreen";
import { parseInboxSearch } from "../features/inbox/model";
import { Legacy } from "./routes/interim";
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
  component: ModelScreen,
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

const glossaryRoutes = segmentRoutes("glossary", GlossaryView);
const entitiesRoutes = segmentRoutes("entities", EntitiesView);
const rulesRoutes = segmentRoutes("rules", RulesView);
const calculationsRoutes = segmentRoutes("calculations", CalculationsView);
const processesRoutes = segmentRoutes("processes", ProcessesView);

// ------------------------------------------------------------------ Scenarios

const scenariosRoute = createRoute({ getParentRoute: () => rootRoute, path: "/scenarios", component: ScenariosScreen });

const scenarioRoute = createRoute({ getParentRoute: () => rootRoute, path: "/scenarios/$id", component: ScenariosScreen });

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
