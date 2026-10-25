# CLAUDE.md — TensAI Design System

You are working with the **TensAI Design System**. This document gives you the operating instructions for any design / engineering work in this repo (or any project that has this repo installed as a skill).

## How to orient yourself

1. **Always** read `SKILL.md` first.
2. Then `README.md` for the full brand guide.
3. Tokens are in `tokens/colors_and_type.css` (CSS variables) and `tokens/tokens.json` (JSON, for tooling).
4. Brand assets in `assets/logos/`.
5. Reference UI implementations in `references/website/` and `references/app/`.
6. Starter scaffolds in `templates/vanilla/` and `templates/react/`.

## The five non-negotiable brand rules

1. **Reserved violet.** `#6D28D9` appears **once per surface** — on the single primary CTA + the brand mark. Never on every link, chip, or eyebrow. Default to midnight (`#0F172A`) for everything else.
2. **The enso is the only ornament.** Don't draw new ones. Don't redraw it as a clean vector circle. Don't tile it. Use it large, once.
3. **No emoji. No exclamation marks. No "Unleash / Supercharge / Game-changer."** Confident, restrained copy. Short sentences. "Driving innovation."
4. **Off-white `#FAFAFA`** for page backgrounds, **never `#FFF`**. **Midnight `#0F172A`** for headlines, never pure black.
5. **The name** is always **TensAI** — capital T, lowercase ens, capital AI.

## How to build something new

When asked to build a new artifact (page, prototype, component):

1. Decide which template fits: `templates/vanilla/` for a one-pager / quick mock; `templates/react/` for anything that needs to grow.
2. Copy the chosen template into the user's project (or alongside it).
3. Copy `tokens/colors_and_type.css` and any required assets from `assets/logos/` into the new project (the templates have `public/` slots for these).
4. Reference `references/website/` and `references/app/` for component patterns — **lift the structure and tokens, not the raw code.** These are JSX + Babel for prototyping; production code should match the target stack.
5. Apply all five rules above. If you find yourself adding a second violet element, stop and pull one back to midnight.

## How to extend the system

Don't invent new tokens silently. If a real need arises (new status color, new radius), say so explicitly and update both `tokens/colors_and_type.css` AND `tokens/tokens.json` in the same change.

## What never to do

- Don't generate AI-imagery as a substitute for the enso.
- Don't add gradients, glows, or "abstract blob" backgrounds.
- Don't use Inter / Roboto / system-ui for display text — use Montserrat (or the eventual TensAI custom typeface).
- Don't add a "Powered by AI" banner anywhere.
- Don't ship visual designs to production without flagging substitutions (typeface, icons) to the user.
