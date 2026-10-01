---
name: Poiesis
description: A local-first, agentic desktop LLM application
colors:
  canvas: "#f3f5f3"
  paper: "#ffffff"
  paper-edge: "#e6eae6"
  paper-edge-2: "#b8c0b8"
  ink: "#17201c"
  ink-muted: "#5a6560"
  ink-faint: "#8a938e"
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
  callout:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "14px"
  footnote:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "12px"
  label:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "11px"
  micro:
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "10px"
  page:
    fontFamily: "Newsreader Variable, Newsreader, Georgia, Times New Roman, serif"
    fontSize: "28px"
  mono:
    fontFamily: "JetBrains Mono, SF Mono, Consolas, monospace"
rounded:
  xs: "3px"
  sm: "5px"
  md: "7px"
  lg: "10px"
  xl: "14px"
spacing:
  "1": "4px"
  "2": "8px"
  "3": "12px"
  "4": "16px"
  "5": "20px"
  "6": "24px"
  "8": "32px"
  "10": "40px"
  "12": "48px"
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
    rounded: "50%"
    size: "32px"
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    rounded: "{rounded.md}"
    height: "32px"
    padding: "0 14px"
  button-secondary:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    height: "32px"
    padding: "0 14px"
  button-danger:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.paper}"
    rounded: "{rounded.md}"
    height: "32px"
    padding: "0 14px"
  card-panel:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.lg}"
  menu:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.lg}"
  menu-row:
    rounded: "{rounded.sm}"
  segmented:
    rounded: "{rounded.md}"
    height: "32px"
  composer:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.xl}"
  chip:
    backgroundColor: "{colors.paper-edge}"
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.sm}"
    padding: "3px 10px"
---

# Design System: Poiesis

## Overview

**Creative North Star: "The Liquid-Crystal Panel"**

Poiesis is one physical panel of monochrome liquid crystal, either read by ambient light ("Daylight") or lit from behind ("Backlit") — never two different apps wearing a light/dark class. Both modes share the same faint green-glass tint in ink and paper, the same restrained radii, and the same hairline grid; only the direction the light travels changes. The panels are flat: they don't sit on top of each other, they're etched into a single sheet and separated by structural dividers, never by color-block. Only what genuinely floats above the sheet (composer, menus, dialogs) takes a soft shadow. The finish is macOS-grade: every interactive surface answers the pointer with a fill and a short eased transition, and things arrive rather than appear.

Rejected: glassmorphism, colorful gradients, drop-shadow-heavy card stacks, pill-shaped text buttons, and any decorative use of the local/cloud accent colors. This is not a "friendly SaaS" surface; it's a quiet, precise reading and working instrument that occasionally shows you a machine is at work.

**Key Characteristics:**
- One panel, two lighting conditions — never two unrelated themes.
- Restrained, nested radii (3/5/7/10/14px); structure comes from hairlines, interaction from ink-derived fills.
- Ink is the only highlight color. Functional color (local/cloud/ok/danger) is a signal, never decoration. Hover, press and selected fills are ink mixed into transparency (`--fill-*`), not new colors.
- Serif for reading, brand and page titles; sans for UI, control and metadata (timestamps, counts, model names — with tabular numerals); mono only for code, paths and machine arguments.
- Two kinds of motion: state motion (breathing, pulsing, orbiting) for the agent's own life, and response motion — short ease-out transitions and arrivals on every interactive surface.

## Colors

The palette is almost monochrome on purpose: cool-neutral paper tones plus a green-black ink, with functional color held in reserve for the few moments the machine needs to speak for itself.

Daylight was recolored to sit closer to the palette of the *Poiesis Harness Shakedown* artifact — the same green-cast neutrals, but cooler and lighter: a clean white panel instead of an off-white one, and the old khaki/olive saturation bred out of the borders and muted text. Backlit (dark) is unchanged.

### Primary
- **Ink** (`#17201c` / dark `#e3e9e0`): the only highlight color in the system. Text, active states, borders-on-focus, filled buttons. 16.7:1 contrast on paper in Daylight.

### Neutral
- **Canvas** (`#f3f5f3` / dark `#0d100e`): the recessed ground the whole shell sits on.
- **Paper** (`#ffffff` / dark `#141814`): panel surface — a clean white in Daylight (was `#f4f6ef`), one step brighter than canvas in Backlit — this is the "lit" layer.
- **Paper Edge** (`#e6eae6` / dark `#232a24`): inner dividers, timeline rails, chip fills.
- **Paper Edge 2** (`#b8c0b8` / dark `#333c34`): panel outlines — the structural grid. Load-bearing: this hairline is what separates the permanent panels, which carry no shadow and no fill, and must stay clearly visible against paper. Deliberately *not* a straight lift of the artifact's border color — that one measures only ~1.4:1 against white, well under what this token held against the old off-white paper (~1.7:1), because the artifact leans on `box-shadow` for card definition and this UI's panels have none (flat by design, see Elevation & Depth below). This value keeps the cooler, less-khaki cast but holds ~1.9:1 against the new white paper.
- **Ink Muted** (`#5a6560` / dark `#8f9a90`): metadata, labels, step verbs. AA body contrast (6.06:1 / 6.15:1).
- **Ink Faint** (`#8a938e` / dark `#6a756c`): secondary labels, hairlines, idle dots. AA large/UI contrast only (3.16:1 / 3.74:1) — never body text.

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

**Character:** Serif carries the brand mark, display moments, document pages and the headings inside an answer; chat answers are set in the sans by default, so the conversation reads in the instrument's own face. Sans carries anything meant to be *operated* — buttons, labels, menus, metadata. The pairing is a newsroom instrument: editorial content in a book face, controls in a working grotesque.

### Hierarchy
- **Display** (500, 21px): brand wordmark, rare display moments.
- **Answer** (400, 15.5px / 1.72 line-height — `--font-message`, `--fs-answer`, `--lh-answer`): chat answer prose. Headings inside an answer are set in the serif, so a long reply keeps an editorial spine. The user's question is 15px/500 in `--font-message`, on a paper card with the `--hairline` outline, `--radius-lg` and `--shadow-xs`.
- **Reading** (400, 17.5px / 1.68 line-height, serif): long-form documents (Workbench markdown). Column capped at 64ch (`--measure`), user-scalable via `--reading-scale` so content reflows rather than truncates. In chat the scale applies to the whole message stream (`zoom` on `.message-stream`), so user turns, tool rows, cards and spacing keep their proportions at every size; elsewhere `.run-text` scales its font alone.
- **Body-LG** (400, 15px, sans): emphasised body text, the palette's search field.
- **Body** (400, 13px, sans): default UI text — buttons, menu items, panel labels.
- **Callout** (14px, sans): field text, the composer input, list titles that lead a row.
- **Footnote / Timeline** (400, 12px, sans): timeline steps, secondary lines, hints, metadata.
- **Label** (600, 11px, sans, sentence case): section labels like `.rail-label` and every page-level section header. Tracked uppercase is reserved for tiny inline badges (kinds, "default").
- **Micro** (10px): counters inside chips and badges only.
- **Page** (500, 28px serif, -0.02em): a route's own `h1`.

Whole pixels only. UI text takes its `font-size` from the `--fs-*` tokens; half-pixel steps (11.5, 12.5, 13.5, 14.5) are not part of the ramp. The exceptions are a handful of one-off display sizes and `em`-relative sizes inside answer prose (headings, code, tables), which scale with the reading size.

### Message font
Settings → Reading lets the user choose the face for chat messages (Inter, Atkinson Hyperlegible, Newsreader, Literata, Source Serif). The store writes it to `--font-message` on the root, and it applies to the **message bodies only**: the user's question text (`.turn-user .body`) and the answer prose (`.run-text`). Everything around them — the "Agent" label, model name, timestamps, steps, plans, cards and chips — stays in the UI sans; code and paths stay mono; headings inside an answer stay serif. It defaults to the UI sans.

### Named Rules
**The Read/Operate Split Rule.** If the user is reading it, it's in a reading face (the serif, or the chosen message font in chat). If the user is clicking, typing into, or scanning it as UI, it's sans. Never mix the two roles within one text run.

## Layout

Poiesis is a fixed-viewport desktop shell (`html, body, #root` locked to `100vh`, no page scroll) laid out on a CSS grid: a collapsible icon/list rail on the left, a topbar spanning the remaining columns, and the active route filling the rest. Panels within a route (composer, side panels, workbench) are self-contained flex/grid regions, not nested cards — depth comes from hairline borders between regions, not from margin-and-shadow card stacking.

Density is compact and editorial: 13px base UI text, spacing on a 4pt grid (`--sp-1` 4px through `--sp-12` 48px), generous only in the reading column (64ch measure, 1.68 line-height) where prose needs room to breathe. The rail collapses to an icon-only strip at narrow widths or on manual toggle, hiding labels and search rather than reflowing to a drawer.

## Elevation & Depth

Poiesis is flat by default. Structural panels — the rail, topbar, and in-page regions — carry no shadow at all; separation is drawn entirely with `--paper-edge-2` hairlines against `--paper`/`--canvas`, and that hairline is structural, not cosmetic. In-stream cards (the user's question, artifact chips, project and add-model cards) may take `--shadow-xs`, the faintest lift, so they read as objects on the sheet. Real elevation exists only on elements that float above the panel plane: the composer, menus and dropdowns, dialogs, the palette, toasts.

### Shadow Vocabulary (tokens in `tokens.css`; Backlit has deeper values plus a faint light rim)
- **`--shadow-xs`**: cards resting on the sheet; buttons with a paper face.
- **`--shadow-sm`**: the composer at rest; a card under the pointer.
- **`--shadow-float`**: menus, dropdowns, the side panel, the focused composer.
- **`--shadow-modal`**: dialogs, the command palette, the lightbox, onboarding.
- **`--ring`**: the soft indigo halo around a focused field.

No literal `box-shadow` values outside `tokens.css`, except inset hairlines and token-coloured rings (a badge's `--canvas` cut-out, a segment's `--hairline` edge).

### Named Rules
**The Flat-by-Default Rule.** Real elevation (`--shadow-sm` and up) belongs only to what floats above the base layout (menu, dialog, toast, the composer). The permanent layout — rail, topbar, route content — stays flat and relies on hairlines for separation. Cards inside it (the user's question, chips, project and add-model cards, tasks) may take only `--shadow-xs`, and lift to `--shadow-sm` under the pointer.

## Shapes

Radii are restrained and nested, macOS proportions: `--radius-xs` (3px) code spans and keycaps; `--radius-sm` (5px) segments and small icon buttons; `--radius` (7px) buttons, fields and rows; `--radius-lg` (10px) cards, panels and menus; `--radius-xl` (14px) the composer and dialogs. A control inside a card is one step smaller than the card so the curves stay concentric. Circles are allowed for the composer's round tool and send buttons and for dots; small status badges may be pills; text buttons are never pill-shaped. Hairline glyphs (a 2px caret, meter bars) may use a 1px corner. Structural borders are 1px `--paper-edge-2`; card borders in the stream use the softer `--hairline`. Checkboxes and radios are drawn from scratch (never native browser controls) to keep the same hairline-box language; checked state fills with `--ink`, never an accent color.

### Named Rules
**The Nested Radius Rule.** Every radius is a token, and nothing exceeds 14px except a true circle or a small badge pill. Emphasis comes from ink, fill and elevation, not from extra curvature.

## Motion

- **Curves:** `--ease-out` for anything arriving or answering the pointer; `--ease-in-out` for layout that moves both ways (rail, dock); `--ease-pop` (expo out, no overshoot) for small things that pop (checkmarks, the composer buttons). No bounce or elastic curves.
- **Durations:** `--dur-fast` (140ms) hover and colour; `--dur` (220ms) menus, popovers, disclosures; `--dur-slow` (360ms) dialogs, panels, layout.
- **Every interactive surface transitions.** `global.css` gives buttons, links, fields and ARIA widgets one transition on background, border, colour, shadow, opacity and transform; nothing snaps.
- **Arrivals, not exits.** `pop-in` / `pop-up` (menus, grown from their trigger), `dialog-in`, `rise-in` (cards, new turns, settings sections), `fade-in`. Leaving is instant.
- **Press:** primary/secondary buttons settle to `scale(0.97)`; cards to `scale(0.99)`; round buttons to `0.92`. Hover lifts cards 1–2px with a deeper shadow.
- `prefers-reduced-motion` zeroes every duration token and every animation.

## Components

Controls are restrained: nothing colored at rest, an ink-derived fill under the pointer, a firmer fill when pressed or selected, and solid ink reserved for the one or two states that truly need to stand out (send button, primary action, checked box).

### Buttons
- **Heights:** `--control-sm` 26px, `--control` 32px, `--control-lg` 36px. Anything sharing a line with another control uses one of these.
- **Icon button** (e.g. `.sidebar-toggle`, `.session-more`): borderless, `--ink-muted` icon; hover `--fill-hover`, press `--fill-press`.
- **Primary** (`.btn-primary`): `--ink` fill, `--paper` text, `--shadow-xs`; hover lightens toward paper.
- **Secondary** (`.btn-secondary`, `.confirm-cancel`): `--paper` face, `--paper-edge-2` border, `--shadow-xs`; hover darkens the border and tints the face.
- **Text** (`.btn-text`): colour-only feedback, flush with the text column.
- **Send** (`.composer .send`): a 32px ink circle; grows 4% on hover, settles on press. The composer's `+` is the same circle without fill and turns into a × while its menu is open.
- **Danger** (`.confirm-go`): filled `--danger` background, `--paper` text; hover brightens via `filter: brightness(1.06)`, never a color swap.

### Segmented control
The one way to pick one of a few (`.segmented`, and the legacy names `.model-tabs`, `.filter-chip`, `.usage-range`, `.self-segment`, `.wb-segment`, `.memory-scope-segment`, `.setting-actions[role=group]`, all drawn in `global.css`): a `--fill-press` track with the chosen segment lifted out as a small paper tile with `--shadow-xs`. Never an inverted ink block.

### Chips
- **Style:** `--fill-quiet` background, `--ink-muted` text, 1px `--hairline` border, `--radius` (attachments) or `--radius-sm` (user-message attachments). Artifact chips are small paper tiles instead: `--paper` face, `--shadow-xs`, lifting 1px on hover.

### Cards / Containers
- **Corner Style:** `--radius-lg` (10px) for cards; `--radius-xl` (14px) for the composer, dialogs and the palette.
- **Background:** `--paper` on `--canvas`.
- **Shadow Strategy:** see Elevation & Depth.
- **Border:** 1px `--hairline` (dialogs, menus, in-stream cards). The composer lifts from `--shadow-sm` to `--shadow-float` on focus rather than drawing a heavier border.
- **Internal Padding:** 20px for dialogs, 16px for cards, 8px for the composer (its round buttons supply their own inset).

### Menus (row-menu, composer drop-up, model and effort pickers)
- **Style:** `--paper` background, 1px `--hairline` border, `--radius-lg`, `--shadow-float`; grows out of its trigger (`pop-in` downward, `pop-up` upward, `transform-origin` at the trigger's corner).
- **Items:** `--radius-sm`, transparent at rest, `--fill-hover` under the pointer, `--fill-press` while pressed; a `.row-menu-sep` hairline divides sections. Icons are SVGs from `Icons.tsx`, never Unicode glyphs.
- **Danger items:** `--danger` text on a faint danger-tinted hover.

### Inputs / Fields
- **Style:** `--paper` background, 1px `--paper-edge-2` border, `--radius`, `--control` height, no inner shadow.
- **Focus:** border shifts to `--ink-faint` and the soft `--ring` halo appears; global `:focus-visible` draws a 2px translucent `--local` outline that follows the element's own radius — the only place `--local` is used purely as a UI signal rather than a model-origin marker.
- **Checkboxes/radios:** hand-drawn 15×15px hairline boxes (see Shapes); checked fill is `--ink`, never an accent.

### Navigation (Rail, settings hub)
- **Style:** one row language everywhere: 30px rows, 8px inset, `--radius`, 13px sans; `--fill-hover` on hover, `--fill-selected` + `font-weight: 500` for where you are. The rail's top actions (New chat, Search, Library, Projects) are rows, not outlined buttons. Icons are 16px SVGs that shift `--ink-muted` → `--ink`. Collapsed state hides labels and centers icons.

### Empty states
A calm, centred note on a `--fill-quiet` ground with `--radius-lg` (`.empty-note`, `.surface .placeholder-note`), in the UI sans. Never a dashed box, never monospace.

### Poiesis Mark (signature component)
The living mark: an SVG membrane/nucleus/orbit drawn entirely in `currentColor` (inherits `--ink`), animated with slow opacity/rotation "breathing" rather than color change, to represent the agent's own state (idle, active, reflecting, healing) without ever introducing a status-light color. Respects `prefers-reduced-motion` by disabling all animation instantly.

## Do's and Don'ts

### Do:
- **Do** keep both themes as one token set (`:root` + `[data-mode="dark"]` overrides), never two independently-designed stylesheets.
- **Do** use hairline borders (`--paper-edge-2`) as the primary structural device between adjacent panels.
- **Do** reserve `--ink` fill for the single most-important action in a given cluster of controls (send, primary confirm).
- **Do** take every radius, size, shadow, duration and fill from the tokens in `tokens.css`.
- **Do** respect `prefers-reduced-motion` on every animation (the codebase already does this globally — preserve it in new work).

### Don't:
- **Don't** use `--local` or `--cloud` as decorative accent colors — they mean "local model" and "cloud model" specifically, nothing else.
- **Don't** add shadows to permanent layout panels (rail, topbar, route content); shadows are reserved for transient floating elements.
- **Don't** introduce a second typeface for either the serif or sans role — Newsreader and Inter are the whole type system.
- **Don't** use native browser checkbox/radio styling; the hand-drawn hairline version is the only correct one.
- **Don't** round any element past 14px (circles and small badge pills excepted), or make a text button pill-shaped.
- **Don't** use Unicode glyphs as icons (▤ ⌁ ◧ ▾ …); they render at a different size and weight in every fallback font. Draw it in `Icons.tsx`.
- **Don't** leave a hover that snaps; if it changes, it transitions.
