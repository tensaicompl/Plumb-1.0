import { createRootRoute, createRoute, createRouter, type RouterHistory } from "@tanstack/react-router";

import { CalculationsPage } from "../features/calculations/CalculationsPage";
import { EntitiesPage } from "../features/entities/EntitiesPage";
import { FindingsPage } from "../features/findings/FindingsPage";
import { GlossaryPage } from "../features/glossary/GlossaryPage";
import { HomePage } from "../features/home/HomePage";
import { ProcessesPage } from "../features/processes/ProcessesPage";
import { QuestionsPage } from "../features/questions/QuestionsPage";
import { ScenariosPage } from "../features/scenarios/ScenariosPage";
import { RequirementDetail } from "../features/requirements/RequirementDetail";
import { RequirementsPage } from "../features/requirements/RequirementsPage";
import { RulesPage } from "../features/rules/RulesPage";
import { SentencesPage } from "../features/sentences/SentencesPage";
import { SessionPanel } from "../features/sessions/SessionPanel";
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

// U2.2: the sentences and the Question inbox with the round manager.
const sentencesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/sentences",
  component: SentencesPage,
});

const questionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/questions",
  component: QuestionsPage,
});

// U2.3: the project room, the real project-mode session panel (scope []).
const sessionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sessions",
  component: function ProjectRoomRoute() {
    return <SessionPanel mode="project" />;
  },
});

// U2.4: the process swimlane view; process selection happens inside the page.
const processesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/processes",
  component: ProcessesPage,
});

// U2.5: the scenarios view; scenario selection and traces stay within the page.
const scenariosRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/scenarios",
  component: ScenariosPage,
});

// U2.6: the analyst views; selection and details stay within each page.
const entitiesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/entities",
  component: EntitiesPage,
});

const findingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/findings",
  component: FindingsPage,
});

const rulesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/rules",
  component: RulesPage,
});

const calculationsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/model/calculations",
  component: CalculationsPage,
});

export const routeTree = rootRoute.addChildren([
  indexRoute,
  requirementsRoute,
  requirementDetailRoute,
  glossaryRoute,
  sentencesRoute,
  questionsRoute,
  sessionsRoute,
  processesRoute,
  scenariosRoute,
  entitiesRoute,
  findingsRoute,
  rulesRoute,
  calculationsRoute,
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
