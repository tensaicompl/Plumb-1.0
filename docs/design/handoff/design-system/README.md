# TensAI Design System

> **Driving innovation.** A modern, focused brand system for TensAI — built around a single, decisive brushstroke and a confident violet.

---

## Using this in Claude Code

This repo is structured as a **Claude Agent Skill** — drop it next to any project (or inside it) and Claude Code will pick up the brand rules automatically.

**Option A — clone alongside your project (simplest)**
```bash
git clone <this-repo-url> tensai-design-system
cd my-tensai-project
claude
> Use the design system at ../tensai-design-system/SKILL.md to build a landing page.
```

**Option B — install as a project skill (recommended for teams)**
```bash
mkdir -p .claude/skills
git clone <this-repo-url> .claude/skills/tensai-design
```
Claude Code auto-discovers skills in `.claude/skills/` — every conversation in that project will follow the TensAI brand rules without you having to ask.

**Option C — git submodule (pin to a version)**
```bash
git submodule add <this-repo-url> .claude/skills/tensai-design
```

Once installed, Claude reads `SKILL.md` → `README.md` → relevant assets, then copies what it needs (logos, tokens) into your project as it builds.

---

## What is TensAI?

TensAI is an AI company whose brand identity is anchored by a single, distinctive mark: an **enso** — a calligraphy brushstroke circle drawn in one breath. The name plays on **天才 (tensai)**, the Japanese word for "genius," combined with **AI**. The wordmark stylizes this with the final two letters set in capitals: **Tens**AI.

The brand's positioning line is **"Driving innovation."**

This design system is a website-ready translation of the brandbook into tokens, type, components, and UI kits that can be used by designers and engineers to build for the TensAI brand consistently.

> **Note.** Outside of the brand book (logo, color palette, tagline), no product, marketing copy, or UI was provided. The UI kit in this system is a **plausible interpretation** of how a TensAI product surface would look — a violet-accented AI workspace — and should be treated as a starting point for the team to refine.

---

## Sources

This system was synthesized from:

- `BrandBook_TensAI/` — the user-attached brand package, containing:
  - `kolory_marki/brand_colors.docx` — full color spec (backgrounds, brand, derived palettes, semantic CSS variables)
  - `LOGO/` — logo variants in SVG, PNG, JPG, PDF (RGB + CMYK), with and without backgrounds
  - `LOGO/TensAI.pptx` — minimal brand placement deck (logo on black / on white / on violet)

The original DOCX and PPTX are preserved in `_source/` in case the reader wants to reference them.

---

## Index

| File / folder | What it is |
|---|---|
| `README.md` | This file — context, fundamentals, manifest |
| `SKILL.md` | Cross-compatible skill manifest for Claude Code / Claude Skills |
| `CLAUDE.md` | Auto-loaded operating instructions for Claude in any project using this system |
| `LICENSE` | All Rights Reserved · TensAI Inc. |
| `tokens/colors_and_type.css` | All design tokens as CSS custom properties |
| `tokens/tokens.json` | Same tokens in JSON, for build tooling |
| `assets/logos/` | Logo lockups + icon-only marks (SVG, PNG, JPG) |
| `references/website/` | Marketing-site reference implementation (lift patterns from here) |
| `references/app/` | Product/app reference implementation |
| `templates/vanilla/` | Minimal HTML starter scaffold |
| `templates/react/` | Vite + React 18 starter scaffold |
| `preview/` | Static cards showcasing the system (visual reference) |

---

## CONTENT FUNDAMENTALS

The brand book itself is sparse on copy, but the few lines we have set a clear tone — **confident, direct, forward-looking, with restraint.** Most of what follows is a system extrapolated from that source plus the visual language.

### Voice
- **Confident, not loud.** The brand whispers expertise: it's the enso, not a starburst.
- **Forward-looking.** Always points at outcomes and momentum ("Driving innovation," not "Helping you with AI").
- **Concise.** Two-word taglines. Short headlines. Body copy that earns every sentence.
- **Calmly technical.** Talk about systems, signals, intelligence — not magic, hacks, or hype.

### Tone by surface
- **Marketing pages** — bold, declarative, single-sentence headlines. Body in 1-2 sentences max per block.
- **Product UI** — neutral, instructive, second-person ("You", "Your project"). No exclamation marks.
- **Empty states & errors** — calm and specific. Tell the user exactly what happened and the one next thing to do.
- **Microcopy** — short, verb-first ("Add file", "Start a thread", "Run").

### Person & casing
- Address the user as **"you."** Refer to the company as **"we"** sparingly.
- **Title Case** for buttons, navigation, and product surfaces.
- **Sentence case** for body copy, descriptions, and toasts.
- **UPPERCASE eyebrows** with wide letter-spacing for section labels.
- The product is always **TensAI** — capital T, lowercase ens, capital AI. Never "Tensai" or "TENSAI."

### Things to avoid
- Emoji in product copy or marketing. (The brand has no place for them — the enso is the single ornament.)
- Exclamation marks. (Confidence doesn't need them.)
- AI-slop phrasing: "Unleash your potential," "Supercharge," "Game-changer," "Powered by AI."
- Hype adjectives: "revolutionary," "next-gen," "cutting-edge."

### Examples

| Don't | Do |
|---|---|
| "🚀 Supercharge your workflow with AI!" | "Move faster. With one system." |
| "Welcome back! What would you like to do today?" | "Start a new thread, or pick up where you left off." |
| "Oh no, something went wrong 😢" | "We couldn't reach the model. Retry in a moment." |
| "Unleash the future of AI" | "Driving innovation." |

---

## VISUAL FOUNDATIONS

### The mark — an enso
The single most important asset in the system is the enso brushstroke circle. **Treat it as sacred.**
- Use it as the logo. Use it as a watermark. Use it ONCE per surface, large.
- Don't draw new ones, don't redraw it as a clean vector circle, don't replicate the strokes in other graphics.
- The brushstroke has visible texture — a true raster-feel even when shown as SVG (the SVG version is rasterized paths from the original brush artwork).

### Color
- **Violet `#6D28D9`** is the brand color — **reserved**. Used for the **one** primary action per surface (the hero CTA, the composer send, the active focus ring) and the brand mark. Avoid violet on every link, chip, or eyebrow.
- **Midnight `#0F172A`** is the dominant accent. Active states, default buttons, link colors, eyebrow labels — all live in midnight, not violet.
- **Off-white `#FAFAFA`** is the page background, never `#FFF`. Slightly softened to feel warmer.
- **Pure black `#000000`** is reserved for the strongest brand statements — full-bleed hero sections where the enso lands in white.
- Status colors (success/warning/danger) are derived to harmonize — emerald, amber, vermillion — not the brand violet.

> **Reserved-violet rule.** On any given surface, count the violet moments. If there's more than one strong violet element (logo aside), you're using it as wallpaper. Pull one back to midnight.

### Type
- Wordmark and display headlines: **Montserrat** — a geometric grotesque. (Substitute for the original logo typeface; flag for the brand team.)
- Body / UI: **Inter** — neutral, screen-optimized, exceptional at small sizes.
- Mono: **JetBrains Mono** for code blocks, model IDs, technical values.
- Scale follows a major-third (1.25×) rhythm. Display sizes use **-0.02em** tracking; body is normal; uppercase eyebrows use **+0.14em**.

### Backgrounds
- Three canonical surfaces: **light off-white**, **midnight**, **pure black**.
- The enso is the only ornament. **No gradients, no meshes, no abstract blobs.**
- Use **violet wash `#EDE9FE`** as a tinted callout — but sparingly, as a single block of color, never as a gradient.
- For full-bleed hero moments, the enso sits on midnight or black as a white or violet brushstroke. Imagery (when used) should be high-contrast, slightly desaturated, with cool/violet color grading.

### Animation
- **Restrained.** Fades and small slides only. No bounces, no spring overshoots, no parallax.
- **Easing:** `cubic-bezier(0.16, 1, 0.3, 1)` for entrances, `cubic-bezier(0.4, 0, 0.2, 1)` for everything else.
- **Durations:** 120ms (hover), 200ms (default), 320ms (page transitions). Never more than 500ms for UI motion.
- The enso itself animates only on hero — a 1.2s draw-on at page load, then still.

### Hover / press states
- **Buttons** — solid primary darkens to `#7C3AED` on hover, `#5B21B6` on press. No scale, no shadow change.
- **Cards** — lift by **+1 shadow level** (e.g., `--shadow-sm` → `--shadow-md`), border darkens 1 step. No scale.
- **Icon buttons** — background fades in (`var(--accent-tint)` at 0 → full opacity).
- **Links** — color shifts to `--accent-hover`. Underline only on hover for body links.

### Borders
- **1px** is the only stroke weight in UI.
- Use `--color-border` (`#E2E8F0`) on light surfaces, `--color-midnight-muted` on dark.
- Avoid double borders. Avoid colored borders except the violet focus ring.
- **Focus ring:** `0 0 0 3px rgba(109, 40, 217, 0.2)` plus a 1px violet outline. Always visible on keyboard navigation.

### Shadows
- Cool, low-spread, multi-layer. Two-stop on each level (a sharp short shadow + a soft long one).
- **Brand accent shadow:** `0 12px 32px rgba(109, 40, 217, 0.28)` — used only on hero CTAs / primary actions in marketing.

### Corner radii
- **4px** dividers / inline tags
- **8px** inputs, small buttons
- **12px** medium buttons, secondary cards
- **16-24px** primary cards, modals
- **999px** chips, pill CTAs, the brand's preferred primary button shape

### Cards
- White (`#FFFFFF`) on the off-white page (`#FAFAFA`) — the **slight contrast is the brand's preferred elevation**.
- 1px `--color-border`, `--radius-lg` (16px), `--shadow-sm`.
- Internal padding: `--space-6` (24px) minimum, `--space-8` (32px) for hero cards.
- On dark surfaces: `--color-midnight-light` cards on `--color-midnight` background.

### Layout
- **Generous whitespace.** The brand carries itself through restraint.
- **Max content width:** 1200px on marketing, 1440px on product.
- **8-column grid on mobile, 12-column on tablet/desktop**, 32px gutter.
- Fixed page chrome (top nav) uses a `backdrop-filter: blur(12px)` on a translucent white/midnight.
- Section spacing: **128px** (`--space-32`) between major marketing sections, **64px** (`--space-16`) inside.

### Transparency & blur
- **Translucent nav** on scroll: white at 80% opacity + 12px blur on light pages; midnight at 80% + blur on dark.
- **Modal overlays:** midnight at 60% opacity, no blur (the modal itself is opaque).
- **Avoid frosted glass cards.** The brand prefers solid, confident surfaces.

### Imagery
- When real imagery is used: high contrast, slightly desaturated, cool/violet color grading. No warm tones.
- Black & white is acceptable and on-brand.
- **Avoid:** stock-tech imagery (glowing brains, blue circuit boards), illustrated metaphors, 3D blobs.

---

## ICONOGRAPHY

The brand book does **not** ship an icon font or icon set — only the logo mark. For this system we use **[Lucide](https://lucide.dev)** as the default UI icon system. It's free, well-curated, has consistent stroke weight (1.5px), and matches the brand's geometric / minimal aesthetic.

> **Substitution flag:** Lucide is a stand-in. If the TensAI team wants a custom icon system, this should be replaced.

### Icon usage rules
- **Stroke weight:** 1.5px (Lucide default). Don't mix weights.
- **Sizes:** 16px (inline / chips), 20px (buttons), 24px (nav / standalone), 32px+ (feature illustrations).
- **Color:** inherit text color via `currentColor`. Accent only for active / brand moments.
- **No filled icons** in the default set. Outlined only. Filled is reserved for active states (e.g., starred / favorited).
- **No emoji** anywhere in the product or marketing.
- **Unicode chars** (•, →, ✕) used sparingly inline. Prefer arrow icons over the literal "→" in buttons.

### Assets shipped
- `assets/logos/icon-purple.svg` · violet enso, no background
- `assets/logos/icon-black.svg` · black enso, no background
- `assets/logos/icon-purple.png` · violet enso PNG
- `assets/logos/icon-black.png` · black enso PNG
- `assets/logos/icon-purple-bg.svg` · violet enso on background
- `assets/logos/icon-black-on-white.svg` · black enso on white
- `assets/logos/logo-lockup-black.svg` · full lockup (icon + wordmark + tagline) in black
- `assets/logos/logo-lockup-purple.svg` · full lockup in violet
- `assets/logos/logo-lockup-on-black.jpg` · full lockup, white on black

### Where to find Lucide
Load from CDN: `https://unpkg.com/lucide@latest`. Inline SVG markup is preferred over icon fonts.

---

## Caveats / known substitutions

1. **Wordmark typeface** — the brand book did not include a font file. The lockup SVGs use rasterized paths, so the logo is safe. For the rest of the system, **Montserrat** is used as the closest free match. *Ask the brand team for the original typeface.*
2. **Icon system** — Lucide is a substitute. *Ask the brand team if they want a custom set.*
3. **Product UI** — no product designs were provided. The `ui_kits/app/` is a reasonable interpretation of how TensAI's product surface would feel given the brand. *Treat as a v0 to refine.*
4. **Body / UI typeface** — Inter is a sensible default; not specified in the brand book.

---

*Brand Color Guide · v1.0 · This system · v1.0*
