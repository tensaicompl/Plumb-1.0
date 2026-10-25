# Handoff: Plumb web app (TensAI Design System)

## Overview
Plumb turns a written requirements document into a verified, testable specification. An analyst imports a document; Plumb (AI-assisted) proposes requirements, glossary, domain model, operations, rules, calculations and scenarios; a human accepts or rejects every proposal; five quality gates (Evidence, Requirements, Model, Decisions, Behaviour) show readiness.

This package contains:
- `spec/UI-SPEC.md` — the full product/UI specification (source of truth for behaviour, API operation names, states, copy). **Read it first.**
- `design-system/` — the TensAI Design System: tokens (CSS + JSON), logos, brand rules, static preview cards.
- `mocks/Plumb App.dc.html` — a high-fidelity, clickable prototype of the whole app.

## About the design files
The files in `mocks/` are **design references created in HTML** — a prototype showing intended look and behaviour, not production code. Recreate them in the target codebase's environment (React, Vue, etc.) with its established patterns. If no codebase exists, **recommended stack: Vite + React 18 + TypeScript, React Router (deep links per spec §3), TanStack Query (API + websocket invalidation), CSS variables from `design-system/tokens/colors_and_type.css`, Lucide icons (`lucide-react`).**

To view the prototype: serve the `mocks/` folder over HTTP (e.g. `npx serve mocks`) and open `Plumb App.dc.html`. It is a single-file component: template markup + a `class Component` logic block at the bottom holding all mock data and interactions. Mock data in that file is verbatim from the HR leave pilot and can be lifted into fixtures.

## Fidelity
**High-fidelity.** Colours, type, spacing, radii and interactions are final within the TensAI system. Recreate pixel-accurately. Not yet designed (build from spec + this system): dark appearance, failing/undecidable scenario runs, write-conflict sheet, Polish strings, a standalone component gallery.

---

## Global shell (all screens)

Layout: full-viewport flex row, `overflow:hidden`. Body bg `#FAFAFA` (`--bg-page`). Base font Inter 14px, colour `#0F172A`.

### Sidebar
- Width **232px** expanded / **64px** collapsed (transition width 200ms `cubic-bezier(0.4,0,0.2,1)`). Bg `#FFFFFF`, right border 1px `#E2E8F0`.
- Header 56px: enso `icon-black.svg` 40×40 + "Plumb" Montserrat 800 18px, tracking −0.02em. Bottom border 1px.
- Nav (padding 12px 8px, gap 2px): Overview, Inbox, Requirements, Model, Scenarios, Quality, Sources. Item: height 36px, padding 0 10px, radius 8px, gap 10px, Lucide icon 18px, Inter 13.5px.
  - Default: transparent, text `#334155`, weight 500. Hover `#F1F5F9`.
  - Active: bg `#0F172A`, text `#FFFFFF`, weight 600.
  - Inbox badge: min-width 22, height 20, pill, JetBrains Mono 600 11px; bg `#F1F5F9` / text `#0F172A` (on active: bg `#334155`, text white). Shows "99+" over 99. Count = pending proposals + open questions + findings needing decision.
- Divider 1px `#E2E8F0`, margin 4px 16px. Then Assistant + Settings (same item style).
- Footer: avatar 32px circle `#0F172A` with initials (Inter 700 12px white), name 13px/600, role 11px `#6B7280` ("Analyst · Reviewer"). Top border 1px.

### Toolbar (height 56px)
Padding 0 20px, gap 12px, bottom border 1px, bg `rgba(255,255,255,0.8)` + `backdrop-filter: blur(12px)`. Left → right:
1. **Project switcher**: "HR Leave Management" Montserrat 600 15px + chevron; truncates with ellipsis first when space is short.
2. **Version indicator**: pill 26px high, 1px border, 12px text. Dot 6px: `#10B981` "Up to date" · `#6D28D9` "Updating…" · `#F59E0B` "Changed — Refresh" (click refreshes). Shows "v42" in mono `#6B7280`.
3. Flexible spacer.
4. **Search trigger**: 34px high, max 260px, radius 8px, bg `#F1F5F9`, border 1px; "Search or run a command" + "⌘K" mono 11px `#9CA3AF`. Opens command palette.
5. **Next action** (the ONE violet element): pill 34px, padding 0 16px, bg `#6D28D9`, hover `#7C3AED`, pressed `#5B21B6`, white Inter 600 13px + arrow-right icon. Label is computed (see Overview priority order), e.g. "Review 7 proposals".
6. **Run step split button**: left half pill-left, 1px border, white, play icon + "Run next step"; while running shows "NN%" and a 2px violet progress bar along its bottom edge. Right half 30px chevron opens a 240px menu (radius 12, `--shadow-lg`, padding 6) listing each step with status meta ("Done" / "Needs review").
7. Assistant toggle icon button 34×34, radius 8.

**Compact (viewport < 1280px):** search becomes a 34px icon-only button; version pill shows only dot + "v42" (status text returns when updating/stale); run button icon-only. Must fit at 1024px.

### Connection banner
Below toolbar when websocket drops: 34px, bg `#FEF3C7`, text `#92400E` 13px/500, centred: refresh icon + "Live connection lost. Reconnecting… Your data refreshes when it's back."

### Inspector (right panel)
Width `clamp(320px, 32vw, 420px)` (Scenarios up to 440px), bg white, left border 1px. Content padding 24px, sections gap 20px. Sticky action bar: top border 1px, padding 14px 20px, buttons 38px pills. Empty state: centred "Select an item" 14px `#6B7280`.

### Overlays
- **Scrim**: `rgba(15,23,42,0.6)`, no blur.
- **Sheet**: 480px wide (matrix sheet 760px), white, radius 20px, padding 24px, `--shadow-xl`, gap 14px. Title Montserrat 600 18px; description 13.5px `#334155`. Reason textarea min-height 96, radius 10, border 1px; focus border `#6D28D9` + ring `0 0 0 3px rgba(109,40,217,0.2)`. Footer right: Cancel (outline pill 38px) + Confirm (pill 38px, bg `#0F172A`, or `#DC2626` for destructive Reject/Discard) with "⌘↩" hint. Confirm disabled (opacity 0.4) until a required reason is typed.
- **Command palette**: 620px, top 12vh, radius 16, `--shadow-xl`. Input row 56px, 15px text, "esc" hint. Groups with uppercase eyebrow labels (11px/600, tracking 0.14em, `#6B7280`); rows min 40px, radius 8, hover `#F1F5F9`: icon · label 13.5px/500 · snippet 12px `#6B7280`. Empty query → Commands + Recent. No results → "No results for “x”. Try a requirement ID like HR-009."
- **Toast**: bottom-centre, 28px from bottom, pill, bg `#0F172A`, white 13px/500, check icon `#6EE7B7`, optional action pill (bg `#1E293B`) "View in Model" / "Review" / "Undo". Auto-dismiss 3.8s.
- **Settings**: 520px sheet, radius 20, padding 28. Grid 120px label / value: Language (segmented English | Polski), Appearance (System | Light | Dark), Project, Database (mono), AI status (dot + text). Info box bg `#F1F5F9`: key is set via `PLUMB_API_KEY` on the server, never in UI.

---

## Screens

### 1. Overview (`/overview`)
Centred column max-width 960px, padding 40px 32px 64px, gap 24px.
1. **AI-off notice** (when `inference_available:false`): bg `#F1F5F9`, radius 12, padding 12×16, info icon + copy + "Open Settings".
2. **Header**: eyebrow "OVERVIEW"; H1 Montserrat 700 40px "HR Leave Management"; meta 13px `#6B7280` "Updated 4 minutes ago by Plumb · 32 requirements from requirements.md". Right: readiness card (white, border, radius 16, `--shadow-sm`, padding 16×20) with 96px ring (stroke 8, track `#E2E8F0`, value `#0F172A` ≥85, `#64748B` ≥70, else `#F59E0B`), centred "86%" Montserrat 700 24px + "ready" 10px; five sub-score bars (label 84px, 4px bar `#F1F5F9` with `#0F172A` fill, mono value): Clarity 92, Completeness 84, Confirmation 78, Consistency 95, Verifiability 81.
3. **Next action card**: bg `#0F172A`, radius 20, padding 28×32. Eyebrow "NEXT ACTION" `#94A3B8`; title Montserrat 600 24px white; sub 14px `#94A3B8`; white pill button 44px (text `#0F172A`). While running: 4px progress bar (`#1E293B` track, `#C4B5FD` fill).
   Priority: failed step "Extract failed — Retry" → pending proposals "Review 7 proposals from the Flows step" → open questions "2 questions are waiting for HR" → stale gate → next step → "Your specification is ready — Export".
4. **Pipeline** card (white, radius 16, padding 24): 9-column grid — Import, Requirements, Glossary & quality, Model, Flows, Calculations & rules, Scenarios, Questions, Ready. Each: 30px circle + label 12px/500 + status 11px.
   - Done: filled `#0F172A`, white check, "Done".
   - Needs review: white, 1.5px `#0F172A` border, count in mono, "Needs review" `#B45309`.
   - Running: bg `#EDE9FE`, icon `#5B21B6`, "NN%".
   - Not started: white, 1px dashed `#CBD5E1`, step number `#9CA3AF`.
   Click opens what the step produced.
5. **Quality gates**: 5-column grid of tiles (white, radius 12, padding 14×16, `--shadow-xs`, hover `--shadow-md` + border `#CBD5E1`). 10px dot + name 14px/600 + code mono 11px `#9CA3AF`; state line 12px/500. Click → `/quality/:gate`.
6. **At a glance**: 4 tiles — Requirements 32 ("2 with issues"), Model "6 entities" ("6 states · 8 operations"), Scenarios 7 ("7 pass"), Open questions. Value Montserrat 700 28px.
7. **Recent decisions** card: rows 12px×24px, top border; 28px initials circle, "**Who** what", time right 12px `#6B7280`. Link "History".

States: **First run** (hero: enso 160px, eyebrow "NEW PROJECT", H1 40px "Import your requirements document", body 16px, dashed drop zone radius 16 padding 40 "Drop a file here, or choose one · .md .txt .docx"), **Running**, **Needs review** (default), **All green** (all steps done, next action Export), **AI unavailable**.

### 2. Inbox (`/inbox`, `/inbox/:id`)
Three panes:
- **Filters** `clamp(180px,16vw,220px)`, white, padding 20px 10px. Title "Inbox" Montserrat 600 20px. Rows 34px radius 8: All · Proposals · Questions · Wording · Issues to decide, then eyebrow "BY ORIGIN": From steps · From questions · From the Assistant. Active: bg `#F1F5F9`, weight 600. Counts mono 11px at 0.7 opacity. Keyboard hint box (bg `#F1F5F9`, 11px).
- **Queue**: header 52px (title 14px/600 + "N waiting" + "Accept all from Flows" outline pill 30px). Rows min-height 56px, padding 8px 20px, bottom border: 30px icon tile (radius 8, `#F1F5F9`), title 13.5px (600 when selected) ellipsised, sub 12px `#6B7280` "Type · Origin", optional "Stale" pill (bg `rgba(245,158,11,0.14)`, text `#B45309`), age 12px. Selected row bg `#F1F5F9`.
- **Inspector** card per type:
  - **New element**: eyebrow "NEW OPERATION · FLOWS STEP", title Montserrat 600 20px, facts grid (110px key `#6B7280` / value 500), Evidence quotes, "What changes in the model" diff (`+` lines mono 12px `#047857`). Actions: Accept (midnight) · Reject · More.
  - **Link**: two side-by-side cards (operation / requirement).
  - **Question**: why-it-matters text, linked chips, typed answer — Yes/No as segmented control (track `#F1F5F9`, selected white + shadow) or a vertical radio list (selected border `#0F172A`); optional rationale textarea; "Routed to HR · asked 2 days ago". Primary "Answer" disabled (`#CBD5E1`) until chosen. Answering converts the item into an **Answer to confirm**.
  - **Wording (EARS)**: Original (removed words struck, red tint `rgba(220,38,38,0.12)` / `#B91C1C`) vs Suggested (added in green tint `rgba(16,185,129,0.14)` / `#047857`); 2×2 slot tiles (Class, Level, Modality, Pattern).
  - **Answer to confirm**: model diff box (header bg `#F1F5F9`; `−` line on `rgba(220,38,38,0.06)`, `+` line on `rgba(16,185,129,0.08)`), actions Accept · Discard.
  - **Issue to decide**: blocker callout (bg `rgba(220,38,38,0.08)`, text `#B91C1C`, rule code mono), both affected requirements side by side, actions Fix (deep link) · Waive with reason.
  - **Stale banner**: bg `rgba(245,158,11,0.12)`, text `#92400E`, "Proposed on an older version (v40)…" + white "Refresh proposal" pill. Accept disabled.
  - **Evidence quote**: 1px border, radius 10, padding 12×14; highlighted span bg `#EDE9FE`; footer "HR-008 · requirements.md" + "Open in source".
  - **Multi-select** (⇧/⌘ click): centred count Montserrat 700 40px, "Accept N" + "Clear", note "Accepted together as one change. If any item is out of date, none are applied."
  - **Empty**: 56px green circle check, "You're all caught up" Montserrat 600 22px, next pipeline action button.
- Behaviour: on accept/reject the row is removed, focus moves to the next row, toast "Operation added to the model · View in Model". Reject/Waive/Discard open the reason sheet (required). Keyboard J/K/A/R; ⌘↩ confirms sheet; Esc closes.

### 3. Requirements (`/requirements/:id`)
List + inspector. Header: H2 Montserrat 700 28px + "32 · from requirements.md". Filter bar: search input 240px/32px, chips (Kind, Status, Has issues, Low readiness, Uncovered) pill 30px outline, "Sort: Document order".
Row min-height 64px, padding 10×24: ID mono 12px `#6B7280` (52px), statement 13.5px clamp 2 lines, kind tag (radius 4, bg `#F1F5F9`, 11px), issue count (alert icon `#B45309`), coverage icons (operation + flask; `#0F172A` covered, `#CBD5E1` not), readiness mono + 20px ring. Item-health error row: "Couldn't load HR-017. Retry" in `#B91C1C`.
Inspector: ID + kind tag + "Accepted" status pill + More menu (Change kind…, Split into two…, Mark as duplicate of…); statement Inter 500 18px selectable; Wording suggestion box with "Accept wording"; Readiness (72px ring + five sub-scores with notes); Quality issues rows (message, rule code mono, "Waive"); Evidence; Linked chips (navigate).

### 4. Model (`/model/...`)
Header H2 "Model" + segmented control (Glossary · Entities · Rules · Calculations · Processes; track `#F1F5F9` radius 10 padding 3; segment 30px radius 8; active white + `0 1px 3px rgba(15,23,42,0.1)`), right: "Coverage" and "Changes +3" outline pills.
- **Glossary**: table card radius 16; columns 200px / 1fr / 220px / 110px — Term (600), Definition (+source ID mono), Aliases chips, Status pill (Confirmed green / Proposed neutral).
- **Entities**: 5-column card grid (selected border `#0F172A` + `--shadow-md`), then detail card: H3 name; attribute table (Attribute · Type·unit as mono dropdown chip · Nullable · Data class tag amber mono e.g. `pii`, `financial`); State diagram: Draft → (Submit leave request) → Submitted → Approved / Rejected / Withdrawn; Approved → (Cancel) → Cancelled. States are pills, edges 1px `#94A3B8` with operation labels 11px.
- **Rules**: left list (260px) with "1 gap" badge; decision table grid (When employment type · and leave type · Then eligible · Source). Uncovered cell: dashed 1px `#F59E0B`, bg `rgba(245,158,11,0.08)`, "Uncovered · Set outcome" → reason sheet.
- **Calculations**: card with name H3 + status pill; expression block bg `#0F172A`, text `#F8FAFC`, JetBrains Mono 14px; plain-language paragraph; Worked example — 7 day tiles 56px (working days filled `#0F172A`, weekend `#F1F5F9`) "→ 5 working days"; "Confirm calculation".
- **Processes**: swimlane canvas 1180×360 (lanes Employee/Manager/System, 120px each, 96px lane label column); tasks 40px high radius 10 (service tasks bg `#F1F5F9`), gateway = rotated square, start/end circles (end 3px border); orthogonal edges `#94A3B8` with arrowheads; selected node 1.5px `#0F172A` + 3px halo. Right 280px panel: node kind eyebrow, name, Performer / Operation / Produces, step list, "Edit steps".
- **Coverage sheet**: CRUD matrix operations × entities, C/R/U/D in mono 600, empty "·" `#CBD5E1`, tooltip "Approve leave request updates Leave request".
- **Changes sheet**: list of Added/Modified items "Version 39 → 42 · 3 added, 1 modified, 0 removed."

### 5. Scenarios (`/scenarios/:id`)
Header H2 + "7 · 7 pass", actions "Confirm all derived" (outline) and "Run all" (midnight). Row min 60px: title 13.5px/500, source 12px, confirmation pill (neutral), verdict pill (Pass: bg `rgba(16,185,129,0.12)` text `#047857`, check icon; Fail: `rgba(220,38,38,0.10)` / `#B91C1C`; Undecidable/Not run: `#F1F5F9` / `#6B7280`).
Inspector: title; Given/When/Then card (64px uppercase label column 11px tracking 0.1em); verdict callout; Trace timeline (20px status circles + 1px connector; step title 500 + mono detail; on fail highlight first diverging step in red with expected vs actual); "Derived from" link; Confirm · Correct expected outcome · Run this one.

### 6. Quality (`/quality`, `/quality/:gate`, `/quality/history`)
Max-width 1040px. H2 + segmented tabs Gates · Issues · History + "Export" outline pill (opens Export sheet with checklist: Functional model, Glossary, Scenarios, Report, Gate report, Decisions → "Download").
- **Gates**: 5 expandable rows (white, radius 14). Row: 12px dot, name 15px/600 + code, state (Passes `#047857` / Blocked `#B91C1C` / Outdated = hollow 2px `#9CA3AF` ring), summary 13px, chevron rotates 180° (200ms). Expanded: groups Fail · Waived · Warn · Pass · Not applicable (eyebrow labels), each rule row: icon, message 500, mono rule code + note (waiver reason / applicability reason), affected item chips (mono 11px). Footer: "Re-check", "Show affected".
- **Issues**: rows with severity (icon + label coloured), message, code · affected, "Waive with reason" or "Waived" pill + "Waived by X: “reason”".
- **History**: table When (150px) · Who (140px) · Decision (+ rationale in quotes 12px). Footer "Times in Europe/Warsaw".

### 7. Sources (`/sources/:id`)
Left 300px: title, dashed import drop zone, upload progress card (filename, %, 3px bar), file row (icon tile, `requirements.md` mono, "Markdown · 38 passages · imported today"). Right: white reading view, article max 720px, 15px/1.75. Passages that became requirements highlighted `#EDE9FE` (hover `#DDD6FE`) with superscript ID; hover tooltip "Became requirement HR-009 · click to open"; click navigates to the requirement.
Error copy: "This file is already imported" · "The file is empty" · "Unsupported file type".

### 8. Assistant panel (from any screen)
Right panel 400px, bg `#FAFAFA`. Header 52px: "Assistant" + scope chip "Scope · HR-015, HR-026" + close. Bubbles radius 16, 13.5px/1.55: user bg `#0F172A` white, right-aligned; Plumb white with 1px border. Inline proposal card (white, radius 12, `--shadow-sm`): eyebrow "PROPOSAL · NEW OPERATION", name, detail, Accept / Reject → becomes "Accepted · added to the model" (`#047857`). Typing indicator: three 6px dots pulsing (1.2s, staggered 0.15s). Composer: white, radius 16, textarea + 32px midnight send button; disabled while a turn is in flight or AI is off ("AI is off, so the Assistant is read-only…").

---

## Interactions & behaviour
- Global keys: ⌘K palette · ⌘1–7 destinations · Esc close overlays · ⌘↩ confirm sheet. Inbox: J/K/A/R (E edit, Space toggle evidence — to implement).
- Motion: hover 120ms; default 200ms `cubic-bezier(0.4,0,0.2,1)`; entrances `cubic-bezier(0.16,1,0.3,1)`; inspector slide 200ms; list-row collapse 150ms; honour `prefers-reduced-motion`.
- Step run: toolbar progress + Overview stepper/next-action progress; on completion toast "<Step> step finished · 3 new proposals · Review".
- Never show controls a role can't use (from `GET /api/me`). Status is never colour-only (icon + text).
- Data/API mapping per screen: see spec §5 (operation names) and §9 (backend gaps).

## State (prototype → app)
route + deep-link params; inbox items (+ `stale`, `requires_reason`, `discardable`); selection + multi-selection; active inbox filter; question answers draft; open sheet {kind, required, onConfirm}; reason text; toast; run job {stage, pct} (poll `get_job` / websocket); palette query; Model segment + selected entity/node; Quality tab + expanded gate; assistant messages + turn-in-flight; app flags `inference_available`, connection state, model version/stale.

## Design tokens
Source: `design-system/tokens/colors_and_type.css` (CSS vars) and `tokens.json`. Key values used:
- Page `#FAFAFA` · card `#FFFFFF` · surface `#F1F5F9` · border `#E2E8F0` · strong border `#CBD5E1`
- Text `#0F172A` / `#334155` / `#6B7280` / `#9CA3AF`
- Midnight `#0F172A`, midnight-light `#1E293B`, subtext-on-dark `#94A3B8`
- Violet `#6D28D9` (hover `#7C3AED`, pressed `#5B21B6`, light `#C4B5FD`, wash `#EDE9FE`) — **one primary action per surface only**
- Status: pass `#10B981` (text `#047857`, tint 12%) · attention `#F59E0B` (text `#B45309`, tint 14%) · blocked `#DC2626` (text `#B91C1C`, tint 10%) · running = violet wash/`#5B21B6` · neutral `#9CA3AF` on `#F1F5F9` · stale = hollow ring
- Type: Montserrat (display 700/600, tracking −0.02em), Inter (UI), JetBrains Mono (IDs, codes, expressions). Sizes used: 40 / 28 / 24 / 22 / 20 / 18 / 16 / 15 / 14 / 13.5 / 13 / 12 / 11. Eyebrow 11px/600 uppercase, tracking 0.14em.
- Spacing 4px base (4, 8, 12, 16, 20, 24, 32, 40, 48, 64). List rows 44 compact / 56 regular.
- Radii: 4 tags · 8 inputs/nav items · 10–12 small cards · 16 cards · 20 sheets/hero card · 999 pills/buttons.
- Shadows: `--shadow-xs/sm/md/lg/xl` (see tokens). Focus ring `0 0 0 3px rgba(109,40,217,0.2)` + 1px violet.
- Scrim `rgba(15,23,42,0.6)`.

## Assets
- `design-system/assets/logos/` — enso marks and lockups (use `icon-black.svg` in sidebar/empty states; never redraw the enso).
- Icons: Lucide, 1.5–1.6px stroke, 16/18/20/24px, `currentColor`. Used: home, inbox, list, layout-grid, flask-conical, shield-check, file-text, message-square, sliders-horizontal, search, check, check-check, x, play, chevron-down, more-horizontal, triangle-alert, circle-help, square-plus, link, pen-line, reply, arrow-right, arrow-up, upload, download, refresh-cw, info, zap, diff.

## Files
- `spec/UI-SPEC.md` — product spec (screens, flows §6, system states §7, a11y §8, gaps §9, deliverables §10).
- `mocks/Plumb App.dc.html` — prototype (template + logic class with all mock data). Open via a local server; `support.js`, `tokens.css`, `assets/` are its runtime deps.
- `design-system/README.md`, `SKILL.md`, `BRAND-RULES.md` — brand rules (content tone, reserved-violet rule, motion, borders).
- `design-system/tokens/` — tokens. `design-system/preview/` — static reference cards for colours, type, buttons, inputs, badges, cards, navbar.

## Suggested Claude Code prompt
> Read `design_handoff_plumb/README.md`, then `spec/UI-SPEC.md` and `design-system/README.md`. Scaffold a Vite + React + TS app using the tokens in `design-system/tokens/colors_and_type.css`. Build the shell (sidebar, toolbar, inspector, sheet, toast, ⌘K palette) first, then each screen in the order Overview → Inbox → Requirements → Model → Scenarios → Quality → Sources → Assistant, matching `mocks/Plumb App.dc.html`. Use mock fixtures from the prototype's logic block until the API is wired.
