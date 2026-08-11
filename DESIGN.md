---
name: Poiesis
description: A local-first, agentic desktop LLM application
colors:
  canvas: "#eceee6"
  paper: "#f4f6ef"
  paper-edge: "#d3d8c9"
  paper-edge-2: "#b9c0ad"
  ink: "#16241c"
  ink-muted: "#556157"
  ink-faint: "#7d8a7f"
  local: "#3d4fa0"
  cloud: "#b5642e"
  ok: "#4a7a5e"
  danger: "#a8453a"
typography:
  display:
    fontFamily: "Newsreader Variable, Newsreader, Georgia, Times New Roman, serif"
    fontSize: "21px"
  reading:
    fontFamily: "Newsreader Variable, Newsreader, Georgia, Times New Roman, serif"
    fontSize: "17.5px"
    lineHeight: 1.68
  body:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "13px"
  body-lg:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "15px"
  label:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "12.5px"
  mono:
    fontFamily: "JetBrains Mono, SF Mono, Consolas, monospace"
rounded:
  sm: "4px"
  md: "6px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "14px"
  lg: "20px"
components:
  button-icon:
    backgroundColor: "transparent"
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.sm}"
    size: "28px"
  button-icon-hover:
    textColor: "{colors.ink}"
  button-send:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    rounded: "{rounded.sm}"
  button-outline:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    padding: "7px 10px"
  button-danger:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.paper}"
    rounded: "{rounded.sm}"
    padding: "6px 14px"
  card-panel:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.md}"
  menu-row:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.sm}"
  chip:
    backgroundColor: "{colors.paper-edge}"
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.sm}"
    padding: "3px 10px"
---

# Design System: Poiesis

## Overview

**Creative North Star: "The Liquid-Crystal Panel"**

Poiesis is one physical panel of monochrome liquid crystal, either read by ambient light ("Daylight") or lit from behind ("Backlit") — never two different apps wearing a light/dark class. Both modes share the same faint green-glass tint in ink and paper, the same near-zero radii, and the same hairline grid; only the direction the light travels changes. The whole surface is flat by design: panels don't sit on top of each other, they're etched into a single sheet and separated by structural dividers, never by shadow or color-block. Nothing about it reads as "app chrome" — it reads as an instrument, calibrated once and left alone.

Rejected: glassmorphism, colorful gradients, drop-shadow-heavy card stacks, rounded-pill buttons, and any decorative use of the local/cloud accent colors. This is not a "friendly SaaS" surface; it's a quiet, precise reading and working instrument that occasionally shows you a machine is at work.

**Key Characteristics:**
- One panel, two lighting conditions — never two unrelated themes.
- Near-zero radius; structure comes from hairlines, not shadows or fills.
- Ink is the only highlight color. Functional color (local/cloud/ok/danger) is a signal, never decoration.
- Serif for reading and brand, sans for UI and control, mono for code and machine-legible marks.
- Motion is biological and rare (breathing, pulsing, orbiting) — never mechanical easing for its own sake.

## Colors

The palette is almost monochrome on purpose: warm-neutral paper tones plus a green-black ink, with functional color held in reserve for the few moments the machine needs to speak for itself.

### Primary
- **Ink** (`#16241c` / dark `#e3e9e0`): the only highlight color in the system. Text, active states, borders-on-focus, filled buttons. 14.8:1 contrast on paper in Daylight.

### Neutral
- **Canvas** (`#eceee6` / dark `#0d100e`): the recessed ground the whole shell sits on.
- **Paper** (`#f4f6ef` / dark `#141814`): panel surface, one step lighter (or, in Backlit, one step brighter) than canvas — this is the "lit" layer.
- **Paper Edge** (`#d3d8c9` / dark `#232a24`): inner dividers, timeline rails, chip fills.
- **Paper Edge 2** (`#b9c0ad` / dark `#333c34`): panel outlines — the structural grid. Load-bearing: this hairline is what separates panels in a system with no shadows or fills, and must stay clearly visible against paper.
- **Ink Muted** (`#556157` / dark `#8f9a90`): metadata, labels, step verbs. AA body contrast (5.96:1 / 6.15:1).
- **Ink Faint** (`#7d8a7f` / dark `#6a756c`): secondary labels, hairlines, idle dots. AA large/UI contrast only (3.32:1 / 3.74:1) — never body text.

### Functional (used sparingly, only where it serves a purpose)
- **Local** (indigo, `#3d4fa0` / dark `#8c97e8`): marks a local-model action or state.
- **Cloud** (oxidized copper, `#b5642e` / dark `#d6884a`): marks a cloud-model action or state.
- **Ok** (`#4a7a5e` / dark `#7fb596`): completed timeline step.
- **Danger** (`#a8453a` / dark `#d6776b`): errors and destructive confirmation only.

### Named Rules
**The One Voice Rule.** Ink is the system's only highlight color. Local, cloud, ok, and danger are functional signals that identify *what kind of thing happened*, not decoration — they appear on a small fraction of any given screen, never as a background wash, gradient, or brand accent. If a design needs an accent color for emphasis alone, the answer is ink, not a new hue.

## Typography

**Display / Reading Font:** Newsreader Variable (serif), with Georgia / Times New Roman fallback.
**UI Font:** Inter (sans), with system-UI fallback.
**Mono Font:** JetBrains Mono, with SF Mono / Consolas fallback.

**Character:** Serif carries anything meant to be *read* — chat prose, the brand mark, display moments. Sans carries anything meant to be *operated* — buttons, labels, menus, metadata. The pairing is a newsroom instrument: editorial content in a book face, controls in a working grotesque.

### Hierarchy
- **Display** (500, 21px): brand wordmark, rare display moments.
- **Reading** (400, 17.5px / 1.68 line-height, serif): chat message prose. Column capped at 64ch (`--measure`), user-scalable via `--reading-scale` so content reflows rather than truncates.
- **Body-LG** (400, 15px, sans): user turn body text, composer input.
- **Body** (400, 13px, sans): default UI text — buttons, menu items, panel labels.
- **Timeline** (400, 12.5px, sans): timeline steps, secondary metadata.
- **Label** (500, 11px, sans, uppercase, +0.06em tracking): section labels like "rail-label".

### Named Rules
**The Read/Operate Split Rule.** If the user is reading it, it's serif. If the user is clicking, typing into, or scanning it as UI, it's sans. Never mix the two roles within one text run.

## Layout

Poiesis is a fixed-viewport desktop shell (`html, body, #root` locked to `100vh`, no page scroll) laid out on a CSS grid: a collapsible icon/list rail on the left, a topbar spanning the remaining columns, and the active route filling the rest. Panels within a route (composer, side panels, workbench) are self-contained flex/grid regions, not nested cards — depth comes from hairline borders between regions, not from margin-and-shadow card stacking.

Density is compact and editorial: 13px base UI text, 6–20px spacing steps, generous only in the reading column (64ch measure, 1.68 line-height) where prose needs room to breathe. The rail collapses to an icon-only strip at narrow widths or on manual toggle, hiding labels and search rather than reflowing to a drawer.

## Elevation & Depth

Poiesis is flat by default. Structural panels — the rail, topbar, and in-page regions — carry no shadow at all; separation is drawn entirely with `--paper-edge-2` hairlines against `--paper`/`--canvas`, and that hairline is structural, not cosmetic. Shadows exist only on the small set of elements that genuinely float above the panel plane at a moment in time: the composer (a floating dock, not a fused toolbar), row menus, the confirm dialog, and toasts. Their job is to say "this is temporarily above everything else," not to add texture to permanent UI.

### Shadow Vocabulary
- **Composer float** (`0 2px 10px rgba(0,0,0,0.05), 0 0 0 1px rgba(0,0,0,0.01)`): the lightest lift, for the one persistent floating element.
- **Menu float** (`0 6px 18px rgba(0,0,0,0.14)`): row menus and dropdowns.
- **Dialog float** (`0 10px 30px rgba(0,0,0,0.18)`): the confirm dialog, the highest element in the stack.

### Named Rules
**The Flat-by-Default Rule.** A panel gets a shadow only if it is transient and positioned above the base layout (menu, dialog, toast, floating composer). Anything that is part of the permanent layout — rail, topbar, route content, in-page cards — stays flat and relies on hairlines for separation.

## Shapes

Radius is near-zero everywhere: `--radius` (4px) for controls, chips, and menus; `--radius-lg` (6px) for the composer and larger cards. This reads as an etched, crisp panel rather than a soft plastic one — nothing in the system should feel "pill-shaped" or heavily rounded. Borders are 1px hairlines in `--paper-edge-2` at rest, shifting to `--ink-faint` on hover/focus rather than gaining weight or color. Checkboxes and radios are drawn from scratch (never native browser controls) to keep the same hairline-box language; checked state fills with `--ink`, never an accent color.

### Named Rules
**The Near-Zero Radius Rule.** Radius never exceeds 6px. A "rounder" treatment for emphasis is not an option in this system — emphasis comes from ink and hairline weight, not curvature.

## Components

Controls are etched and restrained: no fills or color washes at rest, hairline borders that simply darken on interaction, and ink reserved for the one or two states that truly need to stand out (send button, selected item, checked box).

### Buttons
- **Shape:** 4px radius, 1px hairline border in `--paper-edge-2`.
- **Icon button** (e.g. `.icon-btn`, `.sidebar-toggle`): transparent background, `--ink-muted` icon, hairline border only where it needs a visible hit target; hover darkens border to `--ink-faint` and text to `--ink`.
- **Primary / Send** (`.send`): filled `--ink` background, `--paper` text — the one button in the system allowed a solid fill, because it's the single most-repeated committing action.
- **Outline** (`.confirm-cancel`, `.rail-top-btn`): transparent background, `--ink` text, `--paper-edge-2` border; hover darkens border only.
- **Danger** (`.confirm-go`): filled `--danger` background, `--paper` text; hover brightens via `filter: brightness(1.06)`, never a color swap.
- **Active/pressed state:** border darkens to `--ink-faint` and text/icon goes to full `--ink`, sometimes with `font-weight: 500` — never a background fill for a non-primary button.

### Chips
- **Style:** `--paper-edge` background, `--ink-muted` text, `--paper-edge-2` 1px border, 4px radius. Used for attachments, model tags, and user-message attachments.

### Cards / Containers (Composer, Confirm dialog)
- **Corner Style:** 6px radius (`--radius-lg`) for floating cards; 4px for everything else.
- **Background:** `--paper` on `--canvas`.
- **Shadow Strategy:** see Elevation & Depth — only floating cards get a shadow.
- **Border:** 1px `--paper-edge-2`, darkening to `--ink-faint` on focus-within.
- **Internal Padding:** 18–20px for dialogs, 10–14px for the composer.

### Menus (row-menu, composer drop-up menu)
- **Style:** `--paper` background, 1px `--paper-edge-2` (or `--paper-edge`) border, 4px radius, menu float shadow.
- **Items:** transparent at rest, `--paper-edge` fill on hover, no border between items — a `.row-menu-sep` hairline divides sections instead.
- **Danger items:** `--danger` text, same hover fill.

### Inputs / Fields
- **Style:** transparent or `--paper` background, 1px `--paper-edge-2` border, 4px radius, no inner shadow.
- **Focus:** border shifts to `--ink-faint`; global `:focus-visible` also draws a 2px `--local` outline with 2px offset as the accessibility floor — the only place `--local` is used purely as a UI signal rather than a model-origin marker.
- **Checkboxes/radios:** hand-drawn 15×15px hairline boxes (see Shapes); checked fill is `--ink`, never an accent.

### Navigation (Rail)
- **Style:** sans-serif 13px items, `--ink-muted` at rest, `--ink` + `font-weight: 500` when active; icons follow the same muted→ink shift. Collapsed state hides labels and centers icons, keeping the same hairline button language.

### Poiesis Mark (signature component)
The living mark: an SVG membrane/nucleus/orbit drawn entirely in `currentColor` (inherits `--ink`), animated with slow opacity/rotation "breathing" rather than color change, to represent the agent's own state (idle, active, reflecting, healing) without ever introducing a status-light color. Respects `prefers-reduced-motion` by disabling all animation instantly.

## Do's and Don'ts

### Do:
- **Do** keep both themes as one token set (`:root` + `[data-mode="dark"]` overrides), never two independently-designed stylesheets.
- **Do** use hairline borders (`--paper-edge-2`) as the primary structural device between adjacent panels.
- **Do** reserve `--ink` fill for the single most-important action in a given cluster of controls (send, primary confirm).
- **Do** keep radius at 4px for controls and 6px for floating cards; nothing rounder.
- **Do** respect `prefers-reduced-motion` on every animation (the codebase already does this globally — preserve it in new work).

### Don't:
- **Don't** use `--local` or `--cloud` as decorative accent colors — they mean "local model" and "cloud model" specifically, nothing else.
- **Don't** add shadows to permanent layout panels (rail, topbar, route content); shadows are reserved for transient floating elements.
- **Don't** introduce a second typeface for either the serif or sans role — Newsreader and Inter are the whole type system.
- **Don't** use native browser checkbox/radio styling; the hand-drawn hairline version is the only correct one.
- **Don't** round any element past 6px, or make a button pill-shaped.
