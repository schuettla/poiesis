/* The app's icon set. No icon library — these are drawn by hand to match the
 * editorial line the rest of the UI holds, which a general-purpose set like
 * Lucide would not. What was missing was not the drawings but a home for them:
 * with every glyph inline at its call site, the cheapest way to get a folder
 * was to paste one, so the folder existed three times, the gear twice, the
 * refresh twice, and the close cross three times in three different geometries.
 *
 * The shared shape of every icon lives on `Glyph` below: a 20×20 viewBox,
 * `fill: none`, and `stroke: currentColor` so each one inherits `--ink` from
 * whatever it sits in and themes for free. Stroke width is 1.3 unless a call
 * site overrides it — small sizes legitimately need a heavier line to stay
 * readable, and those overrides are kept where they already were.
 *
 * The brand mark (Mark/PoiesisMark) and the growth rings (Self/GrowthRings)
 * are deliberately not here: one is the identity, the other a data
 * visualization, and neither is an icon. */

interface GlyphProps {
  /** Rendered px. The 20×20 viewBox scales to it. */
  size?: number;
  strokeWidth?: number;
  className?: string;
  children: React.ReactNode;
}

/** Stroke and fill sit on the <svg>, not on each path — SVG presentation
 * attributes inherit, so the shapes below carry geometry and nothing else. */
function Glyph({ size = 14, strokeWidth = 1.3, className, children }: GlyphProps) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

/** What every icon here accepts. */
export type IconProps = Omit<GlyphProps, "children">;

export function BookmarkIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M5.5 3.5h9a1 1 0 0 1 1 1V17l-5.5-3.2L4.5 17V4.5a1 1 0 0 1 1-1z" strokeLinejoin="round" />
    </Glyph>
  );
}

export function MessageIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path
        d="M3.5 5.5a1 1 0 0 1 1-1h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H8.5l-3.6 2.8a.5.5 0 0 1-.8-.4V13.5h-.6a1 1 0 0 1-1-1v-7z"
        strokeLinejoin="round"
      />
    </Glyph>
  );
}

export function SettingsIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <circle cx="10" cy="10" r="2.6" />
      <path
        d="M10 2.8v2.3M10 14.9v2.3M17.2 10h-2.3M5.1 10H2.8M15.1 4.9l-1.6 1.6M6.5 13.5l-1.6 1.6M15.1 15.1l-1.6-1.6M6.5 6.5 4.9 4.9"
        strokeLinecap="round"
      />
    </Glyph>
  );
}

/** One folder, everywhere. Previously two near-identical drawings: this
 * rounder one (three copies, byte-identical) and a squarer variant in the Rail
 * and the Workbench project header — the latter with a truncated path that
 * relied on `z` to close a side it never drew. Same icon, so: one shape. */
export function FolderIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path
        d="M2.5 5.5A1.5 1.5 0 0 1 4 4h3.2l1.4 1.8H16a1.5 1.5 0 0 1 1.5 1.5v7.2A1.5 1.5 0 0 1 16 16H4a1.5 1.5 0 0 1-1.5-1.5z"
        strokeLinejoin="round"
      />
    </Glyph>
  );
}

/** The file tree's own folder, which is a different icon rather than a copy of
 * the one above: it opens, and the open state is a distinct shape. */
export function TreeFolderIcon({ open, ...props }: IconProps & { open: boolean }) {
  return (
    <Glyph strokeWidth={1.2} {...props}>
      <path
        d={
          open
            ? "M2.5 15V6A1.5 1.5 0 0 1 4 4.5h3.2l1.4 1.8H15A1.5 1.5 0 0 1 16.5 8H5.6L2.5 15z"
            : "M2.5 6A1.5 1.5 0 0 1 4 4.5h3.2l1.4 1.8H16A1.5 1.5 0 0 1 17.5 8v6A1.5 1.5 0 0 1 16 15.5H4A1.5 1.5 0 0 1 2.5 14z"
        }
        strokeLinejoin="round"
      />
    </Glyph>
  );
}

export function FileIcon(props: IconProps) {
  return (
    <Glyph strokeWidth={1.2} {...props}>
      <path d="M5 3.5h6L15 7.5v9H5z" strokeLinejoin="round" />
      <path d="M11 3.5v4h4" strokeLinejoin="round" />
    </Glyph>
  );
}

/** `PRJ-UI-3`: one file's patch. The page corner a file tab wears, with a plus
 * over a minus inside it, so a diff tab reads as "a file, changed". */
export function DiffIcon(props: IconProps) {
  return (
    <Glyph strokeWidth={1.2} {...props}>
      <path d="M5 3.5h6L15 7.5v9H5z" strokeLinejoin="round" />
      <path d="M10 8.5v3M8.5 10h3M8.5 13.5h3" strokeLinecap="round" />
    </Glyph>
  );
}

/** `COD-UI-5`: run. A plain triangle, pointing the way time goes. */
export function PlayIcon(props: IconProps) {
  return (
    <Glyph strokeWidth={1.3} {...props}>
      <path d="M7 5l8 5-8 5z" strokeLinejoin="round" />
    </Glyph>
  );
}

/** One cross. It was drawn three times: twice as the same geometry written two
 * ways (`M15 5L5 15` and `M15 5 5 15`) and once inset by half a unit. */
export function CloseIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M5 5l10 10M15 5 5 15" strokeLinecap="round" />
    </Glyph>
  );
}

const CHEVRONS = {
  left: "M12 4.5 6.5 10l5.5 5.5",
  right: "M8 4.5 13.5 10 8 15.5",
  up: "M4.5 12.5 10 7l5.5 5.5",
  down: "M4.5 7.5 10 13l5.5-5.5",
};

/** Up and down replace the ▴/▾ text carets the pickers used, which render as
 * a dot-sized smudge at caption size in most fallback fonts. */
export function ChevronIcon({ dir, ...props }: IconProps & { dir: keyof typeof CHEVRONS }) {
  return (
    <Glyph strokeWidth={1.5} {...props}>
      <path
        d={CHEVRONS[dir]}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Glyph>
  );
}

/** The panel toggles on either edge of the top bar: the same frame, with the
 * divider on the side the panel is on. */
export function SidebarIcon({ side, ...props }: IconProps & { side: "left" | "right" }) {
  const x = side === "left" ? "7.7" : "12.3";
  return (
    <Glyph {...props}>
      <rect x="2.5" y="3.5" width="15" height="13" rx="2.5" />
      <line x1={x} y1="3.5" x2={x} y2="16.5" />
    </Glyph>
  );
}

/** The "more" affordance. Filled dots rather than a stroked shape, so it opts
 * out of the inherited stroke. */
export function KebabIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <circle cx="4.5" cy="10" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="10" cy="10" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="15.5" cy="10" r="1.4" fill="currentColor" stroke="none" />
    </Glyph>
  );
}

/** Also solid rather than stroked — at 12px a stroked sparkle turns to mush. */
export function SparkleIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M10 2.5l1.5 4.2 4.2 1.5-4.2 1.5L10 13.9 8.5 9.7 4.3 8.2l4.2-1.5z" fill="currentColor" stroke="none" />
      <path d="M15.2 13.1l.7 1.9 1.9.7-1.9.7-.7 1.9-.7-1.9-1.9-.7 1.9-.7z" fill="currentColor" stroke="none" />
    </Glyph>
  );
}

export function DownloadIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path
        d="M10 3v9m0 0-3.5-3.5M10 12l3.5-3.5M4 14.5v1a1.5 1.5 0 0 0 1.5 1.5h9a1.5 1.5 0 0 0 1.5-1.5v-1"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Glyph>
  );
}

export function ExpandIcon({ expanded, ...props }: IconProps & { expanded: boolean }) {
  return (
    <Glyph {...props}>
      <path
        d={
          expanded
            ? "M17 3.5l-5 5m0-5v5h5M3 16.5l5-5m0 5v-5H3"
            : "M12 3h5v5M8 17H3v-5M17 3l-6 6M3 17l6-6"
        }
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Glyph>
  );
}

/** A child agent, branching off the line that started it — what marks a run
 * tab apart from a file's page corner and an artifact's sparkle. */
export function AgentIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <circle cx="10" cy="4.5" r="1.6" />
      <path d="M10 6.1v3.4M10 9.5 5.5 12M10 9.5 14.5 12" strokeLinecap="round" strokeLinejoin="round" />
      <circle cx="5.5" cy="14" r="1.6" />
      <circle cx="14.5" cy="14" r="1.6" />
    </Glyph>
  );
}

export function SearchIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <circle cx="8.8" cy="8.8" r="5" />
      <path d="M12.5 12.5 16.5 16.5" strokeLinecap="round" />
    </Glyph>
  );
}

export function PlusIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M10 4.5v11M4.5 10h11" strokeLinecap="round" />
    </Glyph>
  );
}

/** The settings sections, one drawing each, on the same 20×20 grid and line as
 * everything above. They were Unicode glyphs (▤ ⌁ ◧ ⚒ …), which come from
 * whatever fallback font has them — each a different weight, size and
 * baseline, so the column of icons never lined up. Keyed by view so the hub
 * and the command palette draw the same one. */
export function SectionIcon({ view, ...props }: IconProps & { view: string }) {
  switch (view) {
    case "settings":
      return <SettingsIcon {...props} />;
    case "models":
      return (
        <Glyph {...props}>
          <path d="M10 2.8 16.5 6.4v7.2L10 17.2l-6.5-3.6V6.4z" strokeLinejoin="round" />
          <path d="M3.5 6.4 10 10l6.5-3.6M10 10v7.2" strokeLinejoin="round" />
        </Glyph>
      );
    case "providers":
      return (
        <Glyph {...props}>
          <path
            d="M6 15.5h8.2a3.3 3.3 0 0 0 .4-6.6 4.8 4.8 0 0 0-9.3 1A2.8 2.8 0 0 0 6 15.5z"
            strokeLinejoin="round"
          />
        </Glyph>
      );
    case "runtime":
      return (
        <Glyph {...props}>
          <rect x="5" y="5" width="10" height="10" rx="1.8" />
          <rect x="8" y="8" width="4" height="4" rx="0.6" />
          <path d="M8 2.8V5M12 2.8V5M8 15v2.2M12 15v2.2M2.8 8H5M2.8 12H5M15 8h2.2M15 12h2.2" strokeLinecap="round" />
        </Glyph>
      );
    case "tools":
      return (
        <Glyph {...props}>
          <path
            d="M12.6 3.4a3.6 3.6 0 0 0-4.4 4.7L3.6 12.7a1.6 1.6 0 0 0 2.3 2.3l4.6-4.6a3.6 3.6 0 0 0 4.7-4.4l-2.2 2.2-2-.5-.5-2z"
            strokeLinejoin="round"
          />
        </Glyph>
      );
    case "skills":
      return (
        <Glyph {...props}>
          <path d="M4 4.5A1.5 1.5 0 0 1 5.5 3H15v11.5H5.5A1.5 1.5 0 0 0 4 16z" strokeLinejoin="round" />
          <path d="M4 16a1.5 1.5 0 0 0 1.5 1.5H15V14.5M7.5 6.5h4.5" strokeLinecap="round" strokeLinejoin="round" />
        </Glyph>
      );
    case "apps":
      return (
        <Glyph {...props}>
          <rect x="3.5" y="3.5" width="5.5" height="5.5" rx="1.4" />
          <rect x="11" y="3.5" width="5.5" height="5.5" rx="1.4" />
          <rect x="3.5" y="11" width="5.5" height="5.5" rx="1.4" />
          <rect x="11" y="11" width="5.5" height="5.5" rx="1.4" />
        </Glyph>
      );
    case "tasks":
      return (
        <Glyph {...props}>
          <circle cx="10" cy="10" r="6.8" />
          <path d="M10 6v4.2l2.8 1.8" strokeLinecap="round" strokeLinejoin="round" />
        </Glyph>
      );
    case "mail":
      return (
        <Glyph {...props}>
          <rect x="2.8" y="4.5" width="14.4" height="11" rx="1.8" />
          <path d="m3.4 5.6 6.6 5 6.6-5" strokeLinejoin="round" />
        </Glyph>
      );
    case "activity":
      return (
        <Glyph {...props}>
          <path d="M2.8 10h3.4l2-5 3.6 10 2-5h3.4" strokeLinecap="round" strokeLinejoin="round" />
        </Glyph>
      );
    case "usage":
      return (
        <Glyph {...props}>
          <path d="M4 16.5V11M8 16.5V6M12 16.5V9M16 16.5V3.5" strokeLinecap="round" />
        </Glyph>
      );
    case "workingdir":
      return <FolderIcon {...props} />;
    case "about":
      return (
        <Glyph {...props}>
          <circle cx="10" cy="10" r="6.8" />
          <path d="M10 9v4.5" strokeLinecap="round" />
          <circle cx="10" cy="6.6" r="0.5" fill="currentColor" />
        </Glyph>
      );
    default:
      return null;
  }
}

export function PaperclipIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path
        d="M15.5 9.5 10 15a3.5 3.5 0 0 1-5-5l6-6a2.3 2.3 0 0 1 3.3 3.3L8.5 13.1a1.2 1.2 0 0 1-1.7-1.7L12 6.3"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Glyph>
  );
}

export function ImageIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <rect x="3" y="4" width="14" height="12" rx="2" />
      <circle cx="7.5" cy="8.2" r="1.3" />
      <path d="m3.5 14 4-3.6 3 2.6 2.5-2 3.5 3" strokeLinejoin="round" />
    </Glyph>
  );
}

/** A live interface beside the log: a frame split into a narrow and a wide
 * pane. */
export function LayoutIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <rect x="3" y="3.5" width="14" height="13" rx="2" />
      <path d="M8 3.5v13M8 9h9" />
    </Glyph>
  );
}

export function PersonIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <circle cx="10" cy="7" r="3" />
      <path d="M4 16.5a6 6 0 0 1 12 0" strokeLinecap="round" />
    </Glyph>
  );
}

export function RefreshIcon(props: IconProps) {
  return (
    <Glyph {...props}>
      <path d="M16 10a6 6 0 1 1-1.8-4.3M16 3v3h-3" strokeLinecap="round" strokeLinejoin="round" />
    </Glyph>
  );
}
