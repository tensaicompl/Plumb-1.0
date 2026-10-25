# Plumb — UI specification for design delivery

**Audience:** Claude Design, which delivers the complete UI in the target design system (handoff
package supplied separately).
**Product state:** working backend with 76 HTTP operations; the current UI is a functional
prototype being replaced.
**Design bar:** Apple-level simplicity. Rich, useful functionality, never complicated.

---

## 0. How to use this document

1. Read §1–§3 first: the product, the principles and the structure. Every screen follows from them.
2. §4 is the component library. Build it first, in the target design system's tokens; every
   screen is composed from it.
3. §5 defines every screen end to end: purpose, layout, data, actions, states, mock content.
4. §6 defines the cross-screen flows to prototype.
5. §7 lists the system states every screen must handle. §8 covers accessibility and
   localisation, and §9 the backend gaps.
6. §10 is the exact deliverable checklist.

Mock content throughout uses the real HR leave pilot: 32 requirements about employee leave
requests. Use it verbatim, so mocks are realistic and reviewable.

---

## 1. The product in one page

Plumb turns a written requirements document into a **verified, testable specification**. A
user imports a document. Plumb, assisted by an AI model, proposes structure: requirements, a
glossary, a domain model (entities, attributes, states), operations, rules, calculations and
test scenarios. **Nothing becomes part of the specification until a human accepts it.**
Ambiguities become questions for a domain expert. Five quality gates tell the user, at a glance,
whether the specification is complete and consistent.

### Who uses it

| Role | Does | Typical session |
|---|---|---|
| **Analyst** (primary) | imports documents, runs steps, reviews and accepts proposals, fixes issues | long, focused working sessions |
| **Reviewer** | accepts or rejects proposals, approves waivers | short review passes |
| **Domain expert** (for example an HR manager) | answers questions in plain language | a few minutes, occasionally |

The backend returns the user's roles (`GET /api/me`: `user`, `roles`, `mode`,
`inference_available`). Show only the actions a role can perform; never show a control that
will be refused.

### Core concepts (use these exact words in the UI)

| Concept | Meaning for the user | UI treatment |
|---|---|---|
| **Source** | an imported document (Markdown, text, Word) | file row with type icon |
| **Evidence** | the exact passage of a source that something comes from | highlighted quote, always one click away |
| **Requirement** | one "shall" statement, with a kind (functional, security…) | the main list item of the product |
| **Proposal** | something Plumb suggests; pending until accepted | card with Accept / Reject |
| **Glossary term** | a defined business word (for example "leave request") | term chip |
| **Model** | entities, attributes, states, operations, rules, calculations, processes | structured views |
| **Question** | an ambiguity routed to an expert, with a typed answer | conversational card |
| **Decision** | a recorded answer or waiver, with who, when and why | audit row |
| **Finding** | a quality problem a check found | issue row with severity |
| **Waiver** | a governed "accept this finding", with a reason | finding state |
| **Scenario** | a concrete test case (Given / When / Then) | three-part card |
| **Run** | the simulated execution of a scenario: pass, fail or undecidable | verdict pill |
| **Gate** | a quality checkpoint: I0, F1, F2, F3, F4 | the five-light status |
| **Step** | one stage of the pipeline (extract, consolidate…) | stepper item |
| **Version** | the project's current revision; everything is versioned | quiet indicator |

### The five gates (name them in plain language; show the code only as secondary)

| Code | Plain name | Passes when |
|---|---|---|
| I0 | **Evidence** | the imported sources are intact and addressable |
| F1 | **Requirements** | requirements are grounded, typed, unambiguous and free of blocking defects |
| F2 | **Model** | the domain and behaviour model is coherent |
| F3 | **Decisions** | every blocking question is answered and every waiver governed |
| F4 | **Behaviour** | the required scenarios run and pass |

---

## 2. Design principles

1. **One obvious next action.** At any moment the user should know what to do next. The Overview
   and the toolbar always show it ("Review 12 proposals", "Answer 2 questions", "Run Model
   step").
2. **Review is the product.** Most of the user's time is spent accepting or rejecting what
   Plumb proposes. Make that fast, calm and keyboard-friendly, like triaging mail.
3. **Evidence is never more than one click away.** Every proposed element can show the source
   passage it came from. Trust comes from provenance.
4. **Progressive disclosure.** Lists show the essentials. Detail lives in an inspector panel.
   Rarely used power actions (split, merge, supersede, waive) live in a "More" menu.
5. **Plain language first, codes second.** "Model check" before "F2"; "Missing performer"
   before `missing_performer`. Codes appear small and secondary, for experts.
6. **Calm status.** Use colour only for meaning: pass, attention, blocked, running. Most of the
   screen is neutral. One accent colour for primary actions.
7. **Nothing changes silently.** Show when data changed underneath the user (stale, conflict)
   and offer one tap to refresh.
8. **Safe by default.** Destructive or governed actions (reject, waive, discard) ask for a short
   reason when the backend requires one, in a small sheet, never a modal maze.
9. **Desktop-first productivity app.** Optimised for 1280–1728 px wide laptops and desktops.
   Usable down to 1024 px. Mobile is out of scope.

---

## 3. Information architecture

Fifteen prototype screens consolidate into **seven sidebar destinations**, plus overlays.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Toolbar: Project ▾ · Next action button · Run step ▾ · Search ⌘K · Avatar     │
├──────────────┬───────────────────────────────────────┬──────────────────────┤
│ Sidebar      │ Main content (list / board / canvas)  │ Inspector (detail)   │
│              │                                       │ slides in on select  │
│ Overview     │                                       │                      │
│ Inbox    12  │                                       │                      │
│ Requirements │                                       │                      │
│ Model        │                                       │                      │
│ Scenarios    │                                       │                      │
│ Quality      │                                       │                      │
│ Sources      │                                       │                      │
│ ──────────── │                                       │                      │
│ Assistant    │                                       │                      │
│ Settings     │                                       │                      │
└──────────────┴───────────────────────────────────────┴──────────────────────┘
```

| Destination | Replaces prototype screens | Job |
|---|---|---|
| **Overview** | Home | where am I, what's next, is it ready |
| **Inbox** | Proposals, Sentences, Questions, Findings (decision side) | everything waiting for a human decision, in one queue |
| **Requirements** | Requirements | the requirement list and its detail |
| **Model** | Glossary, Entities, Rules, Calculations, Processes | the specification's structure, in segments |
| **Scenarios** | Scenarios | test cases, runs and verdicts |
| **Quality** | Gates, Findings (list side), Decisions | the five gates, all findings, the decision history |
| **Sources** | (new; API exists) | imported documents and evidence |
| Assistant (panel) | Sessions | chat with Plumb about the specification; it proposes changes |
| Settings (sheet) | (new) | language, project information, AI status |
| Search (⌘K overlay) | header search | jump to anything |

**URL structure**, which must stay deep-linkable:
- `/overview`, `/inbox`, `/inbox/:proposalId`, `/requirements`, `/requirements/:id`;
- `/model/glossary`, `/model/entities/:id`, `/model/rules/:id`, `/model/calculations/:id`,
  `/model/processes/:id`;
- `/scenarios/:id`, `/quality`, `/quality/:gate`, `/quality/history`, `/sources/:id`.

---

## 4. Component library

Deliver every component with all its variants and states: default, hover, focus, pressed,
disabled, loading, and selected where relevant. Light and dark appearance. Use the design
system's tokens; the names below are semantic roles, not colours.

### 4.1 Foundations (map to the design system)

- **Semantic colour roles:**
  - `accent` (primary action);
  - `status.pass`, `status.attention` (warn), `status.blocked` (fail or blocker),
    `status.running`, `status.neutral` (not applicable or not evaluated);
  - `status.stale` (data outdated);
  - surfaces: base, raised, inspector, overlay;
  - text: primary, secondary, tertiary;
  - separators.
- **Type scale:** large title (screen titles), title, headline, body, callout, footnote, caption,
  plus a monospace style for IDs, codes and expressions.
- **Spacing:** a 4-point base; list row height 44 compact / 56 regular.
- **Radius:** card, sheet, control, chip.
- **Motion:** inspector slide (200 ms ease-out); list-row removal on accept or reject (collapse,
  150 ms); progress shimmer. Respect reduced motion.
- **Icons:** a single consistent set (SF Symbols-like). The icon names needed are listed per
  component.

### 4.2 Shell components

| Component | Description | Variants and states |
|---|---|---|
| **Sidebar** | destinations with icons and badge counts (Inbox count = pending proposals + open questions + findings needing decision) | expanded / collapsed (icons only); active item; badge (none / count / "99+") |
| **Toolbar** | project name with version indicator, **Next action** button, **Run step** split button, search field, avatar | next action can be primary or quiet; run step shows a running state with progress |
| **Version indicator** | small dot plus "Up to date" / "Updating…" / "Changed — Refresh" | live, updating, stale |
| **Inspector panel** | right panel, 360–440 px, with title, subtitle, content and a sticky action bar | empty ("Select an item"), loading, loaded, error; resizable |
| **Sheet** | small anchored modal for reasons, confirmations and forms | one field, multi-field, destructive |
| **Toast** | bottom-centre, auto-dismiss, optional "Undo" or "View" | success, info, error |
| **Command palette** | ⌘K overlay: search plus commands ("Run Model step", "Go to Quality") | results grouped by kind; empty; no results |
| **Connection banner** | top of the content area, when the live connection drops | reconnecting, reconnected (auto-hide) |

### 4.3 Data display

| Component | Description | Variants |
|---|---|---|
| **List row** | leading icon or status, title (one or two lines), secondary line, trailing meta | default, selected, pending (proposal), stale, error-health ("couldn't load this item") |
| **Status pill** | short label with a semantic colour | Accepted, Proposed, Rejected, Superseded, Suspect, Deprecated; Open, Answered, Closed; pass, fail, undecidable; Derived, Confirmed |
| **Gate light** | the five gates as five small lights with plain names; click opens Quality | per gate: pass, fail (blocked), stale (outdated), not evaluated, running |
| **Readiness ring** | 0–100 % ring with an overall number; hover shows the five sub-scores (clarity, completeness, confirmation, consistency, verifiability) | small (list), large (detail) |
| **Evidence quote** | a source passage with the exact range highlighted, source name and "Open in source" | inline, expanded, multiple passages |
| **Kind tag** | requirement class: functional, quality, interface, data, security, operational, compliance, transition, constraint | neutral tags, with an icon per class |
| **Finding row** | severity icon, plain message, rule code (secondary), affected items as chips, action | blocker, error, warning; open, waived |
| **Diff view** | before/after for text (word-level) and for model changes (added, modified, removed counts and lists) | inline, side-by-side |
| **Given/When/Then card** | three labelled sections, each a list of readable facts | compact, expanded; with run verdict |
| **Trace timeline** | step-by-step execution trace of a scenario run | pass, fail (first diverging step highlighted), undecidable (missing semantics listed) |
| **Matrix** | CRUD grid of operations × entities, with C/R/U/D letters | empty cells subtle; click shows the operation |
| **Decision-table grid** | a rule's conditions, its rows (when → then), and highlighted overlaps and uncovered cells | read, editable cell (set the outcome), overlap warning |
| **Process canvas** | swimlane diagram (lanes = performers): start/end, human and service tasks, gateways, flows | read, selected node, validation issue badge |
| **Entity card** | name, attributes (name, type, unit, nullable, classification), relationships, states | compact, full |
| **State diagram** | the states and transitions of an entity, with the triggering operation as the edge label | read, selected |
| **Empty state** | icon, one sentence, one action | per screen (see §5) |
| **Skeleton** | loading placeholders matching each layout | list, card, inspector |

### 4.4 Input and decision components

| Component | Description | Variants |
|---|---|---|
| **Proposal card** | what is proposed (plain summary), why (origin: step, question, assistant), evidence, a diff preview, and **Accept / Reject / More** | create, merge, link, supersede, accept; *stale* ("Proposed on an older version — Refresh"); *requires reason*; *discardable* |
| **Typed answer control** | renders a question's answer form from its schema | Yes/No (segmented); pick one (radio list); pick many (checklist); number with unit; text; cardinality (0..1, 1, 0..\*, 1..\*); unit picker; precision; reference picker (pick an existing operation, calendar…); **fixed value** (read-only "always no", shown as information) |
| **Reason sheet** | one multiline field plus a confirm button, for reject, waive and discard where required | required / optional |
| **Split editor** | split a requirement's text into two or more parts; preview each resulting requirement before committing | preview, confirm, conflict |
| **Merge picker** | merge a glossary term into another; preview before committing | picker, preview, confirm |
| **Classify menu** | change a requirement's class | menu with the nine classes |
| **Attribute editor** | set an attribute's type (Date, Decimal(2), String…), unit and data class (pii, financial…) | inline popovers |
| **Run step control** | split button: primary runs the next step; the menu lists all steps | idle, running (progress, "View"), failed (Retry) |
| **Search field** | instant results while typing | idle, typing, results, none |
| **Segmented control** | switches the Model sub-views | 5 segments |
| **Filter bar** | chips: status, kind, severity, origin, "Needs my decision" | none, active, clear |

---

## 5. Screens, end to end

Each screen lists: **Purpose · Layout · Data · Actions · States · Mock content.** API operation
names are given so every control is backed by a real operation. Where a needed operation is
missing, it is marked **(gap)**; see §9.

### 5.1 Overview — "Where am I, and what's next?"

**Purpose.** A single glance answers three questions: how complete is the specification, what
needs me, and what's the next step.

**Layout.** No sidebar inspector. A centred column (max 960 px):
1. **Header:** project name, last updated time, and a large **readiness ring** with its sub-scores.
2. **Next action card**, prominent. One sentence plus one button. Priority order:
   1. a failed step: "Extract failed — Retry";
   2. pending proposals: "Review 12 proposals";
   3. open questions: "2 questions are waiting for HR";
   4. a stale gate: "Re-run the Model step to update the check";
   5. the next pipeline step: "Run Scenarios";
   6. done: "Your specification is ready — Export".
3. **Pipeline stepper**, horizontal: Import → Requirements → Glossary & quality → Model → Flows
   → Calculations & rules → Scenarios → Questions → Ready. Each step shows its state:
   - not started;
   - running (progress);
   - needs review (count);
   - done (check);
   - failed.

   Clicking a step opens what it produced (filtered Inbox or destination).
4. **Gates row**, with the five gate lights in plain language. Each shows its state and, when not
   passing, one line ("2 blocking issues").
5. **At a glance**, a four-tile grid: Requirements 32 · Model (6 entities, 6 states, 8
   operations) · Scenarios 7 (7 pass) · Open questions 0.
6. **Recent activity**, the last 5 decisions with who and when (link: History).

**Data.**
- `get_readiness`;
- `get_gates` (state per gate: current / stale / not evaluated, with reason);
- `list_proposals` (count);
- `list_questions` (open count);
- `list_requirements` (count);
- `list_entities`, `list_scenarios` (counts and verdicts);
- `list_decisions` (recent);
- `get_model_version`;
- the live events websocket (`/api/events`), which refreshes counts.

**Actions.** The next action (it deep-links). Run step (`run_stage`, then `get_job` for progress).

**States.**
- **First run (empty project):** a hero "Import your requirements document" with a drop zone; this
  is the onboarding.
- **Running:** the stepper item animates; the next-action card shows "Plumb is analysing your
  requirements… 40 %" with View details.
- **All green:** a calm success state; the next action is Export.
- **AI unavailable** (`inference_available: false`): a quiet notice, "AI analysis is off — add an
  API key in Settings", and AI-dependent steps are disabled with an explanation.

**Mock.**
- Project "HR Leave Management"; readiness 86 %.
- Next action: "Review 7 proposals from the Flows step".
- Gates: Evidence ✓ · Requirements ✓ · Model ✓ · Decisions ✓ · Behaviour ✓.

### 5.2 Inbox — "Everything waiting for my decision"

**Purpose.** The heart of the product: one triage queue for every pending human decision. It
should feel like Mail: fast, keyboard-first, satisfying to clear.

**Layout.** Three panes:
1. **Left:** smart filters with counts.
   - All · Proposals · Questions · Wording (EARS suggestions) · Issues to decide.
   - Then grouped by origin: from steps / from questions / from the Assistant.
2. **Middle:** the queue list. Each row shows a type icon, a one-line summary, the origin, age, and
   a *stale* badge when applicable.
3. **Right:** the inspector, holding the full proposal card or question card.

**Item types and their cards.**

| Type | Card content | Primary actions | API |
|---|---|---|---|
| **New element** (create) | "Add operation **Submit leave request**", what it reads and writes, who performs it, evidence passages | Accept · Reject | `accept_proposal` / `reject_proposal` |
| **Merge / Link / Supersede** | side-by-side of both elements and what changes | Accept · Reject | `merge_proposal`, `link_proposal`, `supersede_proposal` |
| **Wording suggestion** (EARS sentence) | original sentence versus the suggested structured sentence, with the slots class, level and modality | Accept · Edit · Reject | `accept_sentence`, `edit_sentence`, `reject_sentence`; `accept_requirement_ears` |
| **Question** | the question in plain language, why it matters (linked items), the typed answer control, a rationale field | Answer | `answer_question` |
| **Answer to confirm** (question-origin proposal) | "HR answered: both start and end dates count" plus the model change it causes (diff) | Accept · Discard | `accept_proposal`, `discard_proposal` |
| **Issue to decide** (finding needing governance) | what's wrong, affected items, options | Fix (deep link) · Waive with reason | `waive_finding` |

**Bulk.** Select several rows (⇧/⌘ click), then "Accept 7" in the inspector. **(gap: atomic set
acceptance — accept-set exists in the CLI only.)** "Accept all from this step" is the main time
saver.

**Keyboard.** J/K move down/up · A accept · R reject (opens the reason sheet if required) · E
edit · Space toggle evidence · ⌘↩ confirm the sheet.

**States.**
- **Empty:** "You're all caught up", with a check illustration and the next pipeline action.
- **Stale item:** a banner on the card, "Proposed on an older version", with a
  **Refresh proposal** button (`list_proposals` shows `stale: true`). Accepting is disabled.
- **Requires reason:** Accept opens the reason sheet (`requires_reason: true`).
- **Discardable:** only items from questions or intake show "Discard" (`discardable: true`).
- **Conflict on accept** (another change happened): "This changed since you looked", showing what
  changed and Review again.
- After accept or reject: the row collapses, focus moves to the next item, and a toast offers
  "View in Model".

**Mock (Flows step, 7 items).**
- Add operation **Submit leave request** — performed by Employee; writes Leave request;
  evidence HR-008, HR-009, HR-030.
- Add operation **Approve leave request** — performed by Manager; produces event *Leave request
  approved*; evidence HR-011, HR-015.
- Add event **Leave request submitted**.
- Question: "Does the requested duration include the end date?" — Yes/No. Linked:
  *RequestedWorkingDays*. Routed to HR.

### 5.3 Requirements — "The source of truth"

**Purpose.** Browse, understand and refine the requirements.

**Layout.** List plus inspector.
- **Toolbar:** search within requirements; filters (kind, status, has issues, low readiness,
  uncovered).
- **Sort:** document order / readiness / issues.
- **Row:** source identifier (HR-001, monospace, secondary), the statement (two lines), kind
  tag, a small readiness ring, issue count, coverage icons (operations, scenarios).

**Inspector (requirement detail).**
1. Statement (large, selectable) with the kind tag and status pill.
2. **Wording suggestion** (if any): the EARS suggestion with Accept (`accept_requirement_ears`).
3. **Readiness**: the ring plus the five sub-scores with one-line explanations
   (`get_requirement_readiness`).
4. **Quality issues**: lint findings (for example "Relative time without anchor: 'within 2
   days'"), each with a Waive action (`waive_requirement_finding`).
5. **Evidence**: the source passage (`EvidenceExcerpt`) and the exact spans, with Open in source.
6. **Linked**: operations, processes and scenarios that implement or verify it (chips; clicking
   navigates).
7. **More menu**:
   - Change kind (`classify_requirement`);
   - Split into two… (`preview_requirement_split`, then `split_requirement`);
   - Mark as duplicate of… (`mark_requirement_duplicate`).

**States.**
- **Empty** (no import yet): "Import a document to see its requirements" plus an Import button.
- **Item health error:** one row can't be loaded; the row shows "Couldn't load HR-017"; the list
  still works.
- **Split preview:** the sheet shows the resulting requirements; Confirm commits; if the project
  changed meanwhile, "Preview is out of date — Preview again".

**Mock.**
- HR-009 "Submitting a valid leave request shall change its status from Draft to Submitted." —
  functional, readiness 92 %, linked: Submit leave request.
- HR-030 "The leave-request submission operation should complete within 2 seconds under normal
  office load." — quality, an issue: "Weak modality 'should'".

### 5.4 Model — "The specification's structure"

**Purpose.** Understand the structure Plumb has built and adjust the few things only a human
can (types, units, rule outcomes, process steps).

**Layout.** A segmented control at the top: **Glossary · Entities · Rules · Calculations ·
Processes**. A secondary toolbar item opens **Coverage** (the CRUD matrix) and **Changes**
(model diff between versions).

#### 5.4.1 Glossary
- **List:** term, aliases (chips), language, status. Search.
- **Inspector:** term, the definition (the requirement statement it comes from, as an evidence
  quote), aliases, where it's used.
- **Actions:**
  - Confirm (`confirm_glossary`);
  - Edit term and aliases (`edit_glossary`);
  - Merge into… (`preview_glossary_merge`, then `merge_glossary`, with a preview diff).
- **Mock:** "leave request" (aliases: request), "annual leave balance", "public holiday",
  "manager decision".

#### 5.4.2 Entities
- **Grid of entity cards** (Leave request, Employee, Manager decision, Public holiday,
  Approval record). Clicking a card opens the detail in place (master–detail).
- **Detail:**
  - Attributes table: name · type · unit · nullable · data class, each editable inline:
    - type (`set_attribute_type`),
    - unit (`set_attribute_unit`),
    - class (`set_attribute_class`);
  - Relationships (target, cardinality);
  - **State diagram** (Draft → Submitted → Approved / Rejected / Withdrawn / Cancelled, with the
    operation on each edge).
- **Mock attributes:** start date · Date · required; end date · Date · required; leave type ·
  String · pii.

#### 5.4.3 Rules
- **List:** rule and decision-table names, with an overlap or gap badge.
- **Detail:** the decision-table grid (conditions as columns, rows when → then).
  - Overlaps are highlighted.
  - "Uncovered" cells are shown as dashed cells with **Set outcome**
    (`set_rule_cell` with a reason).
- **Mock:** "Annual leave eligibility": employment type × leave type → eligible yes/no;
  contractor × annual → no (HR-014).

#### 5.4.4 Calculations
- **List:** calculation name and its expression, rendered readably.
- **Detail:**
  - the expression in monospace;
  - plain-language rendering;
  - **Worked example** (`get_calculation_example`), for example
    "3 Jan – 7 Jan (Mon–Fri), no holidays → 5 working days";
  - Confirm (`confirm_calculation`).
- **Mock:** RequestedWorkingDays = working_days([start date, end date], office calendar,
  inclusive end) × day fraction.

#### 5.4.5 Processes
- **List:** process name, steps, validation status.
- **Detail:** the **swimlane process canvas** (lanes: Employee, Manager, System). Selecting a node
  shows its operation, performers and produced events.
- **Edit steps** (add, remove, reorder) via a step list beside the canvas
  (`mutate_process_steps`).
- **Import** from Mermaid text (`import_process`); **Export** (`export_process`).
- **Empty:** "No processes yet — they appear after the Flows step, or import one."

#### 5.4.6 Coverage (sheet or full view)
- The CRUD matrix (`get_crud_matrix`): operations × entities. Hovering a cell explains it
  ("Approve leave request updates Leave request").

#### 5.4.7 Changes (sheet)
- Compare two versions (`get_model_diff`): added, modified and removed elements grouped by type.
- Default: "since my last visit".

### 5.5 Scenarios — "Does the specification behave correctly?"

**Purpose.** See the test cases Plumb derived, confirm them, run them and understand failures.

**Layout.** List plus inspector.
- **Toolbar:** **Run all** (`run_scenarios`); **Confirm all derived** (`confirm_derived_scenarios`);
  a filter by verdict (pass / fail / undecidable / not run) and by confirmation (derived /
  confirmed).
- **Row:** scenario title, the source (rule row / transition / requirement), confirmation pill,
  verdict pill.

**Inspector.**
1. **Given / When / Then card**, readable, for example:
   - **Given** a Submitted leave request of an employee who reports to the manager;
   - **When** the manager approves it;
   - **Then** its status is Approved and the event *Leave request approved* is produced.
2. **Verdict**: pass / fail / undecidable, with a one-line reason.
3. **Trace timeline** (`get_scenario_run`):
   - on fail: the diff (expected versus actual);
   - on undecidable: the missing semantics ("No calendar coverage for 2027") and a link to fix.
4. **Derived from**: the rule row, transition or requirement (link).
5. **Actions:**
   - Confirm (`confirm_scenario`);
   - Correct the expected outcome (`correct_scenario`, editing *Then*);
   - Run this one.

**States.**
- **Not run yet:** "Run scenarios to see results."
- **Running:** row-level progress.
- **Stale run** (the model changed): verdict greyed with "Out of date — Run again".

**Mock.** 7 scenarios, all pass:
- "Contractor requests annual leave → not eligible".
- "Submitted → Withdrawn by employee before decision".

### 5.6 Quality — "Is it ready?"

**Purpose.** The five gates in depth, every finding, and the full decision history.

**Layout.** Tabs: **Gates · Issues · History.**

**Gates tab.**
- **Five large gate rows**, one per gate:
  - the plain name and code;
  - the state: pass / blocked / outdated / not checked yet, with its reason (for example "needs a
    current Model run");
  - the summary: blockers failed, blockers waived, warnings.
- **Expanding a gate** shows its rules as rows, grouped by state (Fail, Waived, Warn, Pass, Not
  applicable). Each row has:
  - a plain message;
  - the rule code, secondary;
  - the affected items as chips;
  - **why not applicable** (applicability reason);
  - the waiver reference if waived.
- **Data:** `get_gates`.
- **Actions:**
  - Re-check: run the step that owns the gate (`run_stage`);
  - "Show affected" filters Requirements or Model.

**Issues tab.**
- All findings (`list_findings`) with filters (severity, status, family).
- Row actions: Open affected item · Waive with reason (`waive_finding`).
- Waived items show the reason and who decided.

**History tab.**
- The decision log (`list_decisions`): when, who, what (answer, waiver, acceptance), the
  rationale, and a link to the question or proposal.
- Read-only, searchable.
- Also lists **Assumptions** (`list_assumptions`), with Resolve (`resolve_assumption`).

**Export** (toolbar of Quality): the specification files (`export_model`): functional model,
glossary, scenarios, report, gate report, decisions.

**Mock.**
- Requirements (F1) passes, with 1 blocker waived: "HR-023 and HR-024 look like duplicates —
  waived: they notify different outcomes."
- Two non-blocking errors: "Requirements without intent lineage (32)" and "Behaviour without
  acceptance criteria (16)".

### 5.7 Sources — "What it all comes from"

**Purpose.** Import documents and read them with Plumb's evidence overlay.

**Layout.**
- **List:** file name, type, imported date, passages count.
- **Detail:** a reading view of the document (`get_source`, `get_source_spans`) with passages
  that became requirements subtly highlighted. Hovering a passage shows what it became; clicking
  opens it.

**Actions.**
- **Import document**: drag and drop or a file picker; .md, .txt and .docx (`create_source`).
- After an import, offer "Run the Requirements step now".

**States.**
- **Uploading:** progress.
- **Rejected:**
  - "This file is already imported" (`E_SOURCE_EXISTS`);
  - "The file is empty" (`E_SOURCE_EMPTY`);
  - "Unsupported file type".
- **Very large document:** a note that analysis may take several minutes.

**Mock.** `requirements.md` · Markdown · 38 passages · imported today.

### 5.8 Assistant (right-side panel, from any screen)

**Purpose.** Ask Plumb about the specification, or ask it to change something. It answers and
**proposes** changes, which appear as proposal cards in the conversation (and in the Inbox).

**Layout.** A chat panel (420 px):
- scope chips at the top (which requirements the conversation is about);
- message bubbles;
- proposal cards inline (Accept / Reject);
- a "question back" card when the Assistant needs a decision.

**Data and actions.**
- `create_session` (with a scope);
- `create_session_turn` (text and stage);
- `get_session`;
- `get_session_metrics` (turns, proposals).

**States.**
- **AI unavailable:** the panel explains and stays read-only.
- **Turn in flight:** a typing indicator; further input is disabled (one turn at a time,
  `E_SESSION_TURN_IN_FLIGHT`).

**Mock.**
- *User:* "Employees should be able to see their remaining balance."
- *Plumb:* "I can add an operation *View leave balance* performed by Employee, reading Employee
  → annual leave balance." [proposal card]

### 5.9 Settings (sheet)

- **Language:** English / Polski (persisted per browser).
- **Appearance:** System / Light / Dark.
- **Project:** name, identifier, current version, database location (read-only).
- **AI:** status (available / off), model name, and how to add a key (instructions; the key is
  never entered in the UI).
- **About:** version and the export of a support bundle **(gap)**.

### 5.10 Search (⌘K)

- Instant results across requirements, terms, entities, operations, rules, scenarios and
  findings (`search_model`), grouped by kind with a snippet.
- Commands mixed into the results: "Run next step", "Go to Inbox", "Export specification",
  "Switch to dark appearance".
- **Empty query** shows recent items and suggested commands.

---

## 6. End-to-end flows to prototype

Prototype each as a clickable flow with the screens and states in order.

1. **First run, empty to first proposals:**
   1. Overview (empty);
   2. drop `requirements.md`;
   3. upload progress, then "38 passages imported";
   4. "Run Requirements step";
   5. running (stepper animates, toolbar progress);
   6. done, "Review 32 requirements";
   7. Inbox filtered to that step;
   8. "Accept all from this step";
   9. Overview, next step.
2. **Triage session:**
   1. Inbox with 12 items;
   2. keyboard J/A/A/R (with the reason sheet);
   3. accept a merge with a diff preview;
   4. a stale item, Refresh;
   5. "All caught up".
3. **Expert answers a question:**
   1. Inbox, Questions;
   2. the question card "Does the requested duration include the end date?";
   3. Yes plus a rationale;
   4. the answer becomes a proposal, Accept;
   5. the calculation updates (toast "Model updated");
   6. History shows the decision.
4. **Fix a failing gate:**
   1. Overview shows Model blocked;
   2. Quality, Gates, Model expanded;
   3. "Event without consumer: Leave request approved";
   4. Show affected;
   5. Model, Entities;
   6. resolve, then Re-check;
   7. pass.
5. **Waive a finding with governance:**
   1. Quality, Issues;
   2. the duplicate finding;
   3. Waive;
   4. the reason sheet;
   5. the row shows Waived, with who and why;
   6. the F1 gate passes.
6. **Scenario failure investigation:**
   1. Scenarios, filter Fail;
   2. the trace timeline highlights the diverging step;
   3. Correct the expected *Then* or follow the link to the rule;
   4. Run again;
   5. pass.
7. **Assistant-driven change:**
   1. open the Assistant;
   2. ask;
   3. a proposal card;
   4. Accept in the conversation;
   5. the Inbox count does not grow (it is already decided).
8. **Concurrency conflict:**
   1. the user reviews a proposal;
   2. meanwhile a step finishes;
   3. Accept;
   4. the "This changed since you looked" sheet;
   5. Review again.
9. **Export:**
   1. Overview "Ready — Export";
   2. the export sheet, choosing artifacts;
   3. download.

---

## 7. System states every screen must handle

| State | Trigger (backend) | UI |
|---|---|---|
| Loading | any read | skeletons matching the layout; never spinners on whole screens |
| Empty | no items | the screen's empty state, with one action |
| Partial item failure | a list `health` entry for one item | a row-level "Couldn't load this item"; the rest of the list works |
| Read failure | network, 502–504 | inline error with Retry; automatic retry on reconnect |
| Live connection lost | websocket closed | connection banner, "Reconnecting…"; data refreshes on reconnect |
| Stale data | the version changed while viewing | the version indicator shows "Changed — Refresh"; affected lists show a subtle "Updated" pill |
| Write conflict | 412 (the reviewed version is outdated) | "This changed since you looked" sheet with the change summary and "Review again" |
| Governed action | reason required | reason sheet |
| Long-running step | job queued, acquiring, evaluating | toolbar run control with progress; the Overview stepper; a toast on completion |
| Step failed | job failed (error code) | the Overview next-action card shows the plain error and **Retry** **(gap: retry endpoint)** |
| Gate outdated | gate `state: stale` | the gate light is outdated (hollow), with the reason and "Re-check" |
| AI unavailable | `inference_available: false` | a quiet notice; AI steps disabled with an explanation |
| Permission | the role lacks an action | the control is hidden; never a disabled control without an explanation |

Errors always use plain language and offer a next step. The backend error code appears only in
a "Details" disclosure. All error codes are catalogued in the API under
`info.x-plumb-error-codes`.

---

## 8. Accessibility, localisation, input

- **WCAG 2.2 AA**: contrast; focus rings; every control reachable and operable by keyboard; a
  semantic structure for screen readers; status never conveyed by colour alone (icon plus
  text).
- **Keyboard:**
  - global: ⌘K search · ⌘1–7 destinations · ⌘. close the inspector · ⌘↩ confirm a sheet;
  - Inbox: J/K/A/R/E.
- **Localisation:** English and Polish at launch.
  - Design for text 30 % longer than English.
  - Dates and times in the local format; the business time zone (Europe/Warsaw) is shown where
    times are recorded.
- **Reduced motion and increased contrast:** supported.
- **Selectable text everywhere**, so statements and IDs can be copied.

---

## 9. Backend gaps the design may rely on (to be built)

These capabilities exist in the CLI or the domain but have no HTTP operation yet. Design them
as specified; engineering will add the endpoints.

| Needed by | Capability | Today |
|---|---|---|
| Inbox bulk | atomic "accept these N proposals" | CLI `accept-set` |
| Overview, failed step | retry a failed step | CLI `retry` |
| Overview, pipeline | run the Directory step (actors, roles, calendar) | CLI `directory` |
| Inbox, Questions | generate questions from open ambiguities, and create a round per expert | CLI `ask`; `create_round` exists |
| Model | confirm a proposed element generically (states, operations) | CLI `confirm` |
| Quality | materialise findings so they can be governed | CLI `findings materialize` |
| Onboarding | create a project from the UI | CLI `init` (the UI assumes a project) |
| Settings | support bundle export | none |

---

## 10. Deliverables checklist for Claude Design

1. **Component library** (§4): every component with variants and states, light and dark, in the
   target design system, with usage notes.
2. **Screens** (§5) at 1440 × 900, plus 1024 × 768 for the Overview, Inbox and Requirements:
   - **Overview:** first run, running, needs review, all green, AI unavailable.
   - **Inbox:**
     - one mock per item type;
     - list states: empty, stale, requires-reason sheet, conflict sheet;
     - multi-select.
   - **Requirements:** list, detail, split flow, empty, item-health error.
   - **Model:**
     - each of the 5 segments, list and detail;
     - Coverage and Changes sheets;
     - process canvas with selection;
     - rule grid with an uncovered cell.
   - **Scenarios:** list, pass detail, fail detail (trace), undecidable detail, stale run,
     running.
   - **Quality:**
     - Gates (all pass; one blocked, expanded);
     - Issues;
     - History;
     - Export sheet.
   - **Sources:** list, reading view with evidence highlights, upload states.
   - **Assistant panel:** conversation with inline proposals, AI unavailable, turn in flight.
   - **Settings sheet**, **Search overlay** (empty, results, commands).
3. **Prototypes** of the nine flows in §6.
4. **System states** (§7) shown at least once each.
5. **Copy deck**: every label, empty state, error message and confirmation in English, with
   Polish strings for the shell and the Inbox, written in plain language (§2.5).
6. **Redlines and specs** for engineering: spacing, sizes, motion timings, keyboard map,
   breakpoints.
