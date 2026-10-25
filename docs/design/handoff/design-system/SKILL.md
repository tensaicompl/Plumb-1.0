---
name: tensai-design
description: Use this skill to generate well-branded interfaces and assets for TensAI, either for production or throwaway prototypes/mocks/etc. Contains essential design guidelines, colors, type, fonts, assets, and UI kit components for prototyping.
user-invocable: true
---

Read the `README.md` file within this skill, and explore the other available files.

If creating visual artifacts (slides, mocks, throwaway prototypes, etc), copy assets out and create static HTML files for the user to view. If working on production code, you can copy assets and read the rules here to become an expert in designing with this brand.

If the user invokes this skill without any other guidance, ask them what they want to build or design, ask some questions, and act as an expert designer who outputs HTML artifacts _or_ production code, depending on the need.

## Quick orientation

TensAI is an AI company. The brand is built around a single distinctive mark — an **enso** (a Zen brushstroke circle) — and a confident violet. The wordmark capitalizes the trailing "AI": **Tens**AI. Tagline: **"Driving innovation."**

| File | Purpose |
|---|---|
| `README.md` | Full guide — content, visual, iconography fundamentals |
| `CLAUDE.md` | Operating instructions for any project that uses this system |
| `tokens/colors_and_type.css` | All design tokens as CSS custom properties |
| `tokens/tokens.json` | Same tokens in JSON, for build tooling |
| `assets/logos/` | Logo lockups and icon-only marks (SVG + PNG) |
| `references/website/` | Marketing-site recreation (lift patterns from here) |
| `references/app/` | Product/app workspace recreation |
| `templates/vanilla/` | Minimal HTML starter — no build step |
| `templates/react/` | Vite + React 18 starter |
| `preview/` | Static cards showcasing the system |

## Core tokens (don't deviate without reason)

- **Brand:** `#6D28D9` violet (base), `#7C3AED` hover, `#5B21B6` pressed.
- **Neutral dark:** `#0F172A` midnight (use instead of pure black for text).
- **Page bg:** `#FAFAFA` (warm off-white, not `#FFF`).
- **Type:** Montserrat (display, 700/800), Inter (body, 400/500), JetBrains Mono (code).
- **Radii:** 8 (inputs) · 12 (default buttons) · 16 (cards) · 999 (pills, primary CTAs).
- **One ornament:** the enso. No gradients, no emoji, no illustrated metaphors.

## Reserved-violet rule (most important)

Violet (`#6D28D9`) is the brand color, but it should appear at most **once per surface** — on the single primary CTA, plus the brand mark. Everything else — links, eyebrows, chips, active states, secondary buttons, icon badges — lives in **midnight** (`#0F172A`) or gray. If a screen has more than one strong violet element (logo aside), pull one back to midnight.

## When designing

1. **Restraint over flourish.** The brand carries itself through whitespace + the enso. Avoid stacking decorations.
2. **Three canonical surfaces:** off-white (`#FAFAFA`), midnight (`#0F172A`), and pure black (`#000000`).
3. **Confident, short copy.** No exclamation marks. No "Supercharge / Unleash" phrasing. Title case for UI labels, sentence case for body.
4. **Primary CTAs are violet pills.** Use violet for one action per surface — the most important one.
5. **The enso is the hero.** Use it once per surface, large. Don't tile it. Don't redraw it.
