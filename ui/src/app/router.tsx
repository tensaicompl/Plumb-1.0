import { createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router";

// The typed route scaffold. Routes are registered explicitly by the task that adds each page
// (U1.1 mounts the shell and Home; U2.x add their own pages); there is no route discovery.
const rootRoute = createRootRoute({
  component: Outlet,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: () => null,
});

const routeTree = rootRoute.addChildren([indexRoute]);

export const router = createRouter({ routeTree });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
