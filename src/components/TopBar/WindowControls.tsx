import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { inTauri } from "../../lib/api";
import { CloseIcon, MaximizeIcon, MinimizeIcon, RestoreIcon } from "../Icons/Icons";

/** Windows draws the app without its native frame (`decorations: false` in
 * `tauri.windows.conf.json`), so the header is the title bar: it carries the
 * three window buttons and moves the window when dragged. macOS and Linux
 * keep their own frames, and a plain browser has no window to control. */
export function framelessWindow(): boolean {
  return inTauri() && typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);
}

/** Whatever in the header answers a click itself — the rest of the row is
 * the title bar's drag surface. */
const INTERACTIVE = "button, a, input, select, textarea, [role='tab'], [role='menu'], [data-no-drag]";

/** Drag from any empty part of the header; a double-click maximizes or
 * restores, as it does on a native title bar. */
export function onTitleBarMouseDown(e: React.MouseEvent) {
  if (!framelessWindow() || e.button !== 0) return;
  if ((e.target as HTMLElement).closest(INTERACTIVE)) return;
  const win = getCurrentWindow();
  if (e.detail === 2) void win.toggleMaximize();
  else void win.startDragging();
}

export default function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!framelessWindow()) return;
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let alive = true;
    const sync = () => void win.isMaximized().then((m) => alive && setMaximized(m));
    sync();
    void win.onResized(sync).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  if (!framelessWindow()) return null;
  const win = getCurrentWindow();

  // 10px glyphs drawn on the pixel grid, the weight Windows 11 uses for its
  // own caption buttons, so the row reads as the window's and not a toolbar's.
  return (
    <div className="window-controls">
      <button className="wc-btn" aria-label="Minimize" title="Minimize" onClick={() => void win.minimize()}>
        <MinimizeIcon size={10} strokeWidth={2} />
      </button>
      <button
        className="wc-btn"
        aria-label={maximized ? "Restore" : "Maximize"}
        title={maximized ? "Restore" : "Maximize"}
        onClick={() => void win.toggleMaximize()}
      >
        {maximized ? <RestoreIcon size={10} strokeWidth={2} /> : <MaximizeIcon size={10} strokeWidth={2} />}
      </button>
      <button className="wc-btn wc-close" aria-label="Close" title="Close" onClick={() => void win.close()}>
        {/* The shared cross is inset to 5..15; at 20px it spans the same 10px the caption button wants. */}
        <CloseIcon size={20} strokeWidth={1} />
      </button>
    </div>
  );
}
