import { createRootRoute, createRoute, createRouter, type RouterHistory } from "@tanstack/react-router";

import { GlossaryPage } from "../features/glossary/GlossaryPage";
import { HomePage } from "../features/home/HomePage";
import { RequirementDetail } from "../features/requirements/RequirementDetail";
import { RequirementsPage } from "../features/requirements/RequirementsPage";
import { AppShell } from "./AppShell";

// The explicit route tree. Each page is registered by the task that implements it (Hotfix 071);
// there is no route discovery and no placeholder route.
const rootRoute = createRootRoute({
  component: AppShell,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: HomePage,
});

// U2.1: the requirement list and detail, and the glossary.
const requirementsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/requirements",
  component: RequirementsPage,
});

const requirementDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/requirements/$requirementId",
  component: function RequirementDetailRoute() {
    const { requirementId } = requirementDetailRoute.useParams();
    return <RequirementDetail key={requirementId} requirementId={requirementId} />;
  },
});

const glossaryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/glossary",
  component: GlossaryPage,
});

export const routeTree = rootRoute.addChildren([indexRoute, requirementsRoute, requirementDetailRoute, glossaryRoute]);

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
