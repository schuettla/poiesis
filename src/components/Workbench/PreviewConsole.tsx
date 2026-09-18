/** `ART-5`: the preview's console, surfaced.
 *
 * The Canvas preview is not a browser build — it is an `<iframe>` rendered by
 * the OS webview, sandboxed away from the app. That means the page can run, but
 * neither we nor the agent could see anything it printed: a page that threw on
 * line one looked exactly like a page with a black background. Both of the
 * Pac-Man rounds died here — the second one was "black screen, just the header
 * shows", which is one uncaught error the user had no way to read and the agent
 * had no way to ask about.
 *
 * The fix is to instrument the document before it runs: a small script ahead of
 * the page's own, hooking `console.*`, `onerror` and `unhandledrejection` and
 * posting each line up to us. `postMessage` crosses the sandbox boundary in the
 * safe direction — the child can talk to the parent, the parent's objects stay
 * out of reach — so nothing here weakens the sandbox.
 *
 * `ART-6` moved that script to the backend, where `agent/preview.rs` splices it
 * into every *served* artifact. `instrument()` below is the same script for the
 * one case the server can't cover: a plain `.html` file previewed from disk,
 * which has no artifact id and so no URL. The two must keep reporting the same
 * shapes — `usePreviewConsole` and the agent's `check_preview` both read them.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { inTauri, recordArtifactConsole, clearArtifactConsole, type ConsoleEntry } from "../../lib/api";
import { SparkleIcon } from "../Icons/Icons";

export type { ConsoleEntry };

/** Marks our messages so a page posting its own doesn't land in the panel. */
const TAG = "poiesis-preview-console";

/** Kept in the panel. The interesting lines are the newest, and the backend
 * ring is the same size. */
const MAX_ENTRIES = 50;

/** Injected ahead of the page's own scripts. Kept deliberately small and
 * defensive: it runs before anything the model wrote, and a bug in here would
 * break every preview rather than one. */
const BRIDGE = `<script>(function(){
  var TAG=${JSON.stringify(TAG)};
  function render(v,depth){
    try{
      if(v instanceof Error) return v.stack||(v.name+": "+v.message);
      if(typeof v==="string") return v;
      if(typeof v==="function") return "[function "+(v.name||"anonymous")+"]";
      if(v===null||v===undefined||typeof v!=="object") return String(v);
      if(depth>1) return Array.isArray(v)?"[array]":"[object]";
      if(Array.isArray(v)) return "["+v.map(function(x){return render(x,depth+1)}).join(", ")+"]";
      if(v.nodeName) return "<"+String(v.nodeName).toLowerCase()+">";
      return JSON.stringify(v,null,0)||String(v);
    }catch(e){ return "[unserializable]"; }
  }
  function post(level,text,source){
    try{
      // Long lines are the model's own dumps; the tail is rarely the point.
      if(text.length>2000) text=text.slice(0,2000)+" …[truncated]";
      parent.postMessage({tag:TAG,kind:"console",level:level,text:text,source:source||null},"*");
    }catch(e){}
  }
  ["log","info","warn","error","debug"].forEach(function(level){
    var original=console[level];
    console[level]=function(){
      var args=Array.prototype.slice.call(arguments);
      post(level,args.map(function(a){return render(a,0)}).join(" "));
      // The page keeps its own console too: devtools stay useful.
      if(original) try{ original.apply(console,args); }catch(e){}
    };
  });
  // Capture phase, so this also sees a subresource failing to load — an <img>,
  // <script> or <link> that 404s fires \`error\` at the element, never at window,
  // and a missing script is the commonest reason a page is blank.
  window.addEventListener("error",function(e){
    var el=e.target;
    if(el&&el!==window&&el.tagName){
      var s=el.src||el.href||"";
      post("uncaught","failed to load "+el.tagName.toLowerCase()+(s?(" "+s):""));
      return;
    }
    var where=e.lineno?("line "+e.lineno+":"+(e.colno||0)):null;
    post("uncaught",(e.error&&(e.error.stack||e.error.message))||e.message||"script error",where);
  },true);
  window.addEventListener("unhandledrejection",function(e){
    post("uncaught","unhandled promise rejection: "+render(e.reason,0));
  });
  // A preview is a page, not a browser: following a link out of it would
  // replace the artifact with a website, inside a frame the user opened to look
  // at their own work. No CSP directive covers a document navigating itself, so
  // the two ways it happens on purpose are stopped here by hand.
  document.addEventListener("click",function(e){
    var el=e.target;
    var a=(el&&el.closest)?el.closest("a[href]"):null;
    if(!a) return;
    var to;
    try{ to=new URL(a.getAttribute("href"),location.href); }catch(err){ return; }
    if(to.origin===location.origin) return;
    e.preventDefault();
    post("error","blocked a link to "+to.href+" — a preview can't navigate away from itself.");
  },true);
  window.open=function(u){
    post("error","blocked window.open("+(u||"")+") — a preview can't open windows.");
    return null;
  };
})();</script>`;

/** Splice the bridge in ahead of everything the page brings with it.
 *
 * It has to land after the doctype — a document whose first node is a `<script>`
 * renders in quirks mode, which would change the layout of the very page we are
 * trying to debug — and before any other script, or the hooks miss the errors
 * that fire during load, which are the ones that matter most. */
export function instrument(html: string): string {
  const doctype = html.match(/^\s*<!doctype[^>]*>/i);
  if (doctype) {
    return html.slice(0, doctype[0].length) + BRIDGE + html.slice(doctype[0].length);
  }
  return BRIDGE + html;
}

/** How many times we will put a page back before concluding it means it.
 * A page that navigates itself on every load would otherwise be a loop between
 * the frame and this hook, and a loop nobody can see is worse than a preview
 * that has given up and said so. */
const MAX_REVERTS = 3;

/** Did the document that just loaded announce itself?
 *
 * Every document the preview server hands out says hello at parse time, before
 * its own `load` fires. So by the time a load has been seen, a document of ours
 * has always contributed a hello — and one that hasn't is a page the frame
 * navigated *itself* to. Comparing running totals rather than timestamps is
 * what makes this hold for the nastiest case: a page whose first script sets
 * `location.href`, where the foreign document arrives milliseconds later. */
export function loadIsForeign(hellos: number, loads: number): boolean {
  return hellos < loads;
}

/** Collect what the preview prints, and forward it to the agent side.
 *
 * `frame` is the iframe the entries must come from: the sandbox gives the page
 * an opaque origin, so origin cannot identify it and the source window is what
 * we check instead. */
export function usePreviewConsole(
  frame: React.RefObject<HTMLIFrameElement | null>,
  artifactId: string | undefined,
  /** Changes whenever the content does, so a re-render clears the old page's
   * output rather than blaming the new code for the old code's errors. */
  contentKey: string,
  /** `ART-6`: the URL this frame is meant to be showing, for a served artifact.
   * `null` for a file preview rendered inline, which has nowhere to be put back
   * to — there the in-page guards are the whole defence. */
  src: string | null = null
) {
  const [entries, setEntries] = useState<ConsoleEntry[]>([]);
  // Batched, because a page in a render loop can print faster than React can
  // reconcile, and each one would otherwise be its own IPC call.
  const pending = useRef<ConsoleEntry[]>([]);
  const timer = useRef<number | null>(null);
  // Running totals for the navigation guard, plus how often we have already
  // put this frame back.
  const hellos = useRef(0);
  const loads = useRef(0);
  const reverts = useRef(0);

  useEffect(() => {
    setEntries([]);
    pending.current = [];
    // A new version of the page has earned a fresh set of attempts: the last
    // one may have been navigating away precisely because of the bug just fixed.
    reverts.current = 0;
    if (artifactId && inTauri()) clearArtifactConsole(artifactId).catch(() => {});
  }, [contentKey, artifactId]);

  const flush = useCallback(() => {
    timer.current = null;
    const batch = pending.current;
    pending.current = [];
    if (!batch.length) return;
    setEntries((prev) => [...prev, ...batch].slice(-MAX_ENTRIES));
    if (artifactId && inTauri()) recordArtifactConsole(artifactId, batch).catch(() => {});
  }, [artifactId]);

  const note = useCallback(
    (entry: ConsoleEntry) => {
      pending.current.push(entry);
      if (timer.current === null) timer.current = window.setTimeout(flush, 120);
    },
    [flush]
  );

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      if (!e.data || e.data.tag !== TAG) return;
      // Only the frame we are showing. Another artifact's preview, or anything
      // else on the page, is not ours to report.
      if (frame.current && e.source !== frame.current.contentWindow) return;
      if (e.data.kind === "hello") {
        hellos.current += 1;
        return;
      }
      note({
        level: e.data.level,
        text: String(e.data.text ?? ""),
        source: e.data.source ?? undefined,
      });
    };
    window.addEventListener("message", onMessage);
    return () => {
      window.removeEventListener("message", onMessage);
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = null;
    };
  }, [frame, note]);

  // `ART-6`'s navigation guard. A sandboxed frame can still navigate *itself*
  // — no CSP directive covers it, and from out here the frame's URL is
  // unreadable across origins. What is readable is that a document loaded and
  // never said hello, which is enough: put the artifact back, and say so in the
  // console so both the user and the agent learn the page tried to leave.
  useEffect(() => {
    const el = frame.current;
    if (!el || !src) return;
    const onLoad = () => {
      loads.current += 1;
      const seen = loads.current;
      // A beat after `load`, so a hello still in flight has arrived.
      window.setTimeout(() => {
        if (!loadIsForeign(hellos.current, seen)) return;
        // Absorb the load that brought no hello, or every later check would
        // measure against a total that can never catch up.
        hellos.current = seen;
        if (reverts.current >= MAX_REVERTS) {
          note({
            level: "error",
            text:
              "This page keeps navigating away from itself. I've stopped putting it back — " +
              "fix whatever is setting location, then reopen the preview.",
          });
          return;
        }
        reverts.current += 1;
        note({
          level: "error",
          text:
            "The page navigated itself away from the preview. I put your artifact back. " +
            "A preview can't browse — show an address as text instead of sending the frame to it.",
        });
        // A fresh query, because assigning the identical URL is not reliably a
        // navigation, and this one has to be.
        if (frame.current) frame.current.src = `${src}${src.includes("?") ? "&" : "?"}r=${reverts.current}`;
      }, 300);
    };
    el.addEventListener("load", onLoad);
    return () => el.removeEventListener("load", onLoad);
  }, [frame, src, note]);

  const clear = useCallback(() => {
    setEntries([]);
    pending.current = [];
    if (artifactId && inTauri()) clearArtifactConsole(artifactId).catch(() => {});
  }, [artifactId]);

  return { entries, clear };
}

/** Is this line the page failing, rather than the page talking? */
function isFailure(e: ConsoleEntry): boolean {
  return e.level === "error" || e.level === "uncaught";
}

/** The message the Fix button sends.
 *
 * Names the artifact by id as well as title so the model reaches for
 * `update_artifact` on the right row rather than guessing, and quotes the error
 * verbatim — the whole value of this button is that the model gets the exact
 * text the page produced instead of the user's paraphrase of a black screen. */
export function fixPrompt(entry: ConsoleEntry, title: string, artifactId: string): string {
  const where = entry.source ? ` (${entry.source})` : "";
  return (
    `The preview of "${title}" (artifact id ${artifactId}) reported this:\n\n` +
    `[${entry.level}] ${entry.text}${where}\n\n` +
    `Read the artifact, work out what causes this, and fix it with update_artifact. ` +
    `Don't create a new artifact.`
  );
}

/** The strip under the preview. Collapsed by default so a working page is just
 * the page — but it opens itself the moment something goes wrong, because an
 * error nobody is told about is the whole problem this is here to fix. */
export function ConsolePanel({
  entries,
  onClear,
  onFix,
  fixDisabled,
}: {
  entries: ConsoleEntry[];
  onClear: () => void;
  /** Hand this error to the agent. Absent for a file preview, which has no
   * artifact behind it for `update_artifact` to change. */
  onFix?: (entry: ConsoleEntry) => void;
  /** A turn is already running — sending a second one would interleave. */
  fixDisabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const errors = entries.filter(isFailure).length;
  const list = useRef<HTMLDivElement | null>(null);
  // Opened by an error, not by every log line: a page that chatters in
  // `console.log` shouldn't keep stealing half the panel.
  const wasErrors = useRef(0);
  useEffect(() => {
    if (errors > wasErrors.current) setOpen(true);
    wasErrors.current = errors;
  }, [errors]);
  useEffect(() => {
    if (open && list.current) list.current.scrollTop = list.current.scrollHeight;
  }, [entries, open]);

  if (!entries.length) return null;

  return (
    <div className={`preview-console ${open ? "open" : ""}`}>
      <div className="pc-head">
        <button
          className="pc-toggle"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          title={open ? "Hide the console" : "Show the console"}
        >
          <span className={`pc-caret ${open ? "open" : ""}`} aria-hidden="true">
            ›
          </span>
          Console
          <span className="pc-count">{entries.length}</span>
          {errors > 0 && (
            <span className="pc-errors">
              {errors} error{errors === 1 ? "" : "s"}
            </span>
          )}
        </button>
        {open && (
          <button className="pc-clear" onClick={onClear} title="Clear the console">
            Clear
          </button>
        )}
      </div>
      {open && (
        <div className="pc-list" ref={list} role="log">
          {entries.map((e, i) => (
            <div key={i} className={`pc-line pc-${e.level}`}>
              <span className="pc-level">{e.level}</span>
              <span className="pc-text">{e.text}</span>
              {e.source && <span className="pc-source">{e.source}</span>}
              {/* Only on the lines that are actually a failure: a `console.log`
                  has nothing to fix, and a Fix button on every line would make
                  the ones that matter harder to find, not easier. */}
              {onFix && isFailure(e) && (
                <button
                  className="pc-fix"
                  onClick={() => onFix(e)}
                  disabled={fixDisabled}
                  aria-label="Ask the agent to fix this error"
                  title={
                    fixDisabled
                      ? "Wait for the current answer to finish"
                      : "Ask the agent to fix this"
                  }
                >
                  {/* The same spark the app uses for "the agent does this" —
                      one glyph, because the line it sits on is already long
                      and the error text is what should be read. */}
                  <SparkleIcon size={12} />
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
