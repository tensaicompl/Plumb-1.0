# Building a screen in the new shell

A note for the screen lanes (Overview, Inbox, Requirements, Model, Scenarios, Quality, Sources,
Assistant). The shell, the design foundation and the UI kit are done; each lane replaces one
destination's interim route with its screen. Read `docs/design/handoff/README.md` (your screen's
section) and `docs/design/handoff/spec/UI-SPEC.md` §5 first, and match the prototype
`docs/design/handoff/mocks/Plumb App.dc.html` and `docs/design/handoff/screens/*.png`.

## Where things live

| What | Where |
|---|---|
| Your screen | `src/features/<screen>/` (for example `src/features/inbox/InboxScreen.tsx` with `inbox.test.tsx`) |
| UI kit | `src/components/ui/` (import from `../../components/ui`) |
| Tokens and Tailwind theme | `src/styles/tokens.css` (light and derived dark), `src/styles/app.css` (`@theme`) |
| Routes | `src/app/router.tsx` |
| Shell (sidebar, toolbar, palette, settings, export, Assistant) | `src/app/AppShell.tsx`, `src/app/shell/` |
| Shared work reads (proposals, open questions, findings, gates) | `src/app/work.ts` |
| Next action, pipeline seam, stage runs | `src/app/nextAction.ts`, `src/app/pipeline.ts`, `src/app/run.ts` |
| API client, ETag reads, versioned writes, events | `src/api/` |
| Copy | `src/i18n/en.json` and `src/i18n/pl.json` |

## Styling

- Use the semantic classes only, never hex values: surfaces `bg-page`, `bg-card`, `bg-surface`;
  text `text-fg`, `text-fg-2`, `text-fg-3` (and `text-fg-3-surface` for secondary text on a
  `bg-surface` background: plain `text-fg-3` there is below WCAG AA); borders `border-line`,
  `border-line-strong`; the midnight control `bg-primary text-on-primary`; status
  `bg-pass-tint text-pass-fg` (and `attention`, `blocked`, `running`, `neutral`); dark hero cards
  `bg-hero text-on-hero text-hero-sub`.
- Type: `font-display` (Montserrat, with `tracking-display`) for titles, `font-sans` (Inter) for UI,
  `font-mono` for IDs and codes. Sizes: `text-h1` 40, `text-h2` 28, `text-headline` 24,
  `text-title-lg` 22, `text-title` 20, `text-title-sm` 18, `text-lead` 16, `text-subhead` 15,
  `text-body` 14, `text-callout` 13.5, `text-footnote` 13, `text-caption` 12, `text-eyebrow` 11.
- Radii `rounded-tag` 4, `rounded-control` 8, `rounded-field` 10, `rounded-tile` 12,
  `rounded-card` 16, `rounded-sheet` 20, `rounded-pill`. Shadows `shadow-xs` … `shadow-xl`.
  Motion `duration-[120ms]` hover, `duration-200` default, `ease-standard`, `ease-entrance`;
  `animate-shimmer`, `animate-inspector-in`. Reduced motion is handled globally.
- Violet (`bg-accent`) is the ONE primary action of a surface. The toolbar's Next action already
  uses it; inside a screen, primary actions are midnight (`Button variant="primary"`), except where
  the handoff puts the screen's own single violet action.
- Dark appearance follows from the tokens. Check your screen with Settings → Appearance → Dark.

## The kit

`Button` (primary · accent · outline · destructive · ghost · inverse; sizes 30/34/38/44),
`IconButton`, `StatusPill`, `GateLight` / `GateDot`, `ReadinessRing`, `Eyebrow`, `Card`, `ListRow` /
`ListRowError`, `KindTag`, `MonoId`, `Kbd`, `IconTile`, `EvidenceQuote`, `DiffLines`,
`SegmentedControl` / `SegmentedLinks`, `FilterChip`, `EmptyState`, `Skeleton` / `SkeletonGroup` /
`SkeletonList`, `Avatar`, `EnsoMark`, `Tooltip`, `Menu`, `Sheet` / `ReasonSheet`, `toast()`,
`WithInspector`. Each has a typed props interface in its file; `display.test.tsx` and
`interactive.test.tsx` show usage. Add to the kit only what two screens need; otherwise keep it in
your feature folder.

## Layout and states

- A route renders inside `<main>` (a flex column). Put exactly one `h1` on the screen.
- List + detail screens use `WithInspector` (`inspector={selected ? <Detail /> : null}`,
  `actions={<Button …/>}`, `inspectorKey={id}`); the main column scrolls on its own.
- Loading uses skeletons matching the layout (`SkeletonList`, `Skeleton`), never a whole-screen
  spinner. Empty uses `EmptyState` (one sentence, one action). A list `health` entry renders a
  `ListRowError` while the rest of the list works. Read failures use `ReadFailure` plus a Retry button calling the query's `refetch` (reads already
  retry 502–504 twice and refresh on reconnect).
- Never show a control the role cannot use (`useMe()` and `hasRole` in `src/app/auth.ts`).
- Status is never colour-only: pills, gate lights and verdicts carry text and an icon.

## Data and writes

- Read with TanStack Query and the generated client: `readReviewed(apiClient.GET(...))` keeps the
  read's `ETag` (`Reviewed<T>`). Reuse the shared queries in `src/app/work.ts` for proposals, open
  questions, findings and gates (same keys and shapes, so the shell and your screen share one
  request).
- Every write goes through `useVersionedMutation(write, reviewedEtag)`: the If-Match is the ETag
  of the data the user reviewed (never a fresh version read). A 412 opens the shell's "This changed
  since you looked" sheet and invalidates the queries; do not retry or merge.
- Governed actions (reject, waive, discard) open a `ReasonSheet` (`reasonRequired` from the
  proposal's `requires_reason`; `destructive` for reject and discard; ⌘↩ confirms, Esc cancels).
- After a write, confirm with `toast({ message, action })` ("Operation added to the model · View in
  Model"); toasts last 3.8 s.
- Design events already invalidate every query (`src/api/useEvents.ts`); do not subscribe again.
- The Next action (`useNextAction()`) and the Run step control live in the shell; the Overview
  card reuses `useNextAction()` (`title`, `description`, `cta`, `progress`) and `useRunNextAction()`.
  Pipeline step states come from `usePipeline()`; plug `GET /api/pipeline` in there when it lands.

## Copy

- Every visible string comes from the catalogues (`t`, `tf` for `{placeholders}`, `tc` for plural
  families with `.one/.few/.many/.other` keys); lint rejects JSX literals. Add each key to `en.json`
  and `pl.json` (the build checks parity). Sentence case, no exclamation marks, plain language
  first and codes second.

## Tests

- Test-first with Vitest and Testing Library; answer HTTP with MSW (`server.use(...)` from
  `src/test/msw.ts`) and return reads with `etagJson(body, init, etag)` so writes can be checked
  against the reviewed ETag. `src/test/msw.ts` already answers the shell's own reads with an empty
  project (`SHELL_READS`); when a test asserts which reads a page makes, leave those out with
  `isShellRead`.
- Render through the real router: `createAppRouter(createMemoryHistory({ initialEntries: [path] }))`.
- `axeViolations()` from `src/test/axe.ts` must return `[]`; `npm run test:coverage` keeps
  `src/features/**` and `src/components/**` at 80 % or more.

## Registering your route

Routes are explicit in `src/app/router.tsx` (no discovery, no placeholder routes). Replace your
destination's interim component with your screen and keep the URLs (spec §3), all deep-linkable:

| Screen | URLs | Interim today | Old folders it replaces (delete them, and their tests, when your screen covers them) |
|---|---|---|---|
| Overview | `/overview` (`/` redirects) | `features/home/HomePage` | `features/home` |
| Inbox | `/inbox`, `/inbox/$id` (`?filter=…`) | `InboxInterim`: `ProposalsPage`, `QuestionsPage`, `SentencesPage` | `features/proposals`, `features/questions`, `features/sentences` (move `TypedAnswerControl` usage into the inspector) |
| Requirements | `/requirements`, `/requirements/$id` | `RequirementsPage`, `RequirementDetail` | `features/requirements` |
| Model | `/model/glossary`, `/model/entities`, `/model/rules`, `/model/calculations`, `/model/processes`, each with `/$id` | `ModelInterim` + the five pages | `features/glossary`, `features/entities`, `features/rules`, `features/calculations`, `features/processes` |
| Scenarios | `/scenarios`, `/scenarios/$id` | `ScenariosPage` | `features/scenarios` |
| Quality | `/quality`, `/quality/$gate`, `/quality/issues`, `/quality/history` | `QualityInterim`: `GatesPage`, `FindingsPage`, `DecisionsPage` | `features/gates`, `features/findings`, `features/decisions` |
| Sources | `/sources`, `/sources/$id` | `SourcesInterim`, `SourceInterim` (no old page; the contract has no source list yet) | none (remove the interim components) |
| Assistant | the shell's right panel (`AssistantPanel`) | `features/sessions/SessionPanel` | `features/sessions` (keep the panel frame in `src/app/shell/Panels.tsx`) |

When the last route of an interim component is replaced, delete it from
`src/app/routes/interim.tsx`. Shared old components (`src/components/*.tsx` outside `ui/`) go when
no screen imports them.
