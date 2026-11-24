import type { ReactNode } from "react";

/** The phone's icons. Same 20-unit grid and thin stroke as the desktop's `Icons.tsx`, so both apps draw alike. */
function Glyph({ size = 20, strokeWidth = 1.5, children }: { size?: number; strokeWidth?: number; children: ReactNode }) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={strokeWidth} aria-hidden="true">
      {children}
    </svg>
  );
}

type Props = { size?: number };

export const BackIcon = (p: Props) => (
  <Glyph {...p}>
    <path d="M12.5 4 6.5 10l6 6" strokeLinecap="round" strokeLinejoin="round" />
  </Glyph>
);

export const ComposeIcon = (p: Props) => (
  <Glyph {...p}>
    <path d="M9 4H5.5A1.5 1.5 0 0 0 4 5.5v9A1.5 1.5 0 0 0 5.5 16h9a1.5 1.5 0 0 0 1.5-1.5V11" strokeLinecap="round" />
    <path d="m14.2 3.3 2.5 2.5L10 12.5l-3 .5.5-3z" strokeLinejoin="round" />
  </Glyph>
);

export const MicIcon = (p: Props) => (
  <Glyph {...p}>
    <rect x="7.5" y="2.5" width="5" height="9" rx="2.5" />
    <path d="M4.5 9.5a5.5 5.5 0 0 0 11 0M10 15v2.5" strokeLinecap="round" />
  </Glyph>
);

export const MicOffIcon = (p: Props) => (
  <Glyph {...p}>
    <rect x="7.5" y="2.5" width="5" height="9" rx="2.5" />
    <path d="M4.5 9.5a5.5 5.5 0 0 0 11 0M10 15v2.5M3 3l14 14" strokeLinecap="round" />
  </Glyph>
);

export const SendIcon = (p: Props) => (
  <Glyph {...p} strokeWidth={1.8}>
    <path d="M10 16V4.5M5 9.5l5-5 5 5" strokeLinecap="round" strokeLinejoin="round" />
  </Glyph>
);

export const StopIcon = (p: Props) => (
  <Glyph {...p}>
    <rect x="5.5" y="5.5" width="9" height="9" rx="1.5" fill="currentColor" stroke="none" />
  </Glyph>
);

export const ImageIcon = (p: Props) => (
  <Glyph {...p}>
    <rect x="3" y="4" width="14" height="12" rx="2" />
    <circle cx="7.5" cy="8.2" r="1.3" />
    <path d="m3.5 14 4-3.6 3 2.6 2.5-2 3.5 3" strokeLinejoin="round" />
  </Glyph>
);

export const CloseIcon = (p: Props) => (
  <Glyph {...p}>
    <path d="M5 5l10 10M15 5 5 15" strokeLinecap="round" />
  </Glyph>
);

export const MoreIcon = (p: Props) => (
  <Glyph {...p}>
    <circle cx="4.5" cy="10" r="1.2" fill="currentColor" stroke="none" />
    <circle cx="10" cy="10" r="1.2" fill="currentColor" stroke="none" />
    <circle cx="15.5" cy="10" r="1.2" fill="currentColor" stroke="none" />
  </Glyph>
);

export const ChevronIcon = (p: Props) => (
  <Glyph {...p}>
    <path d="m7.5 4 6 6-6 6" strokeLinecap="round" strokeLinejoin="round" />
  </Glyph>
);
