import { createRootRoute, createRoute, createRouter, type RouterHistory } from "@tanstack/react-router";

import { HomePage } from "../features/home/HomePage";
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

export const routeTree = rootRoute.addChildren([indexRoute]);

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
