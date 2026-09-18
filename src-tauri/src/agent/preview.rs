//! `ART-6`: the artifact preview gets a real origin.
//!
//! Until now an HTML artifact was rendered by splicing its source into a
//! `srcdoc` iframe. A `srcdoc` frame inherits the *host page's* CSP, and ours is
//! `default-src 'self'` (`tauri.conf.json`) — so an artifact could not load a
//! font, a CDN script, or an image from anywhere. It also had an opaque origin,
//! which costs it `localStorage`, ES modules, and any subresource of its own.
//! Every artifact had to be one self-contained file with no dependencies, and
//! the model had no way to find out that was why its page was blank.
//!
//! So the document is served instead, over a loopback HTTP server bound to
//! `127.0.0.1` on an ephemeral port, behind a per-run token. That gives it a
//! real origin, and — because the response carries its own CSP header — a
//! policy of *its* own rather than the app's.
//!
//! One server, not two: the Canvas iframe and `check_preview`'s headless Chrome
//! both load the same URL. A second mechanism (a Tauri custom URI scheme for the
//! iframe) would have been marginally simpler for the UI alone, but Chrome
//! cannot reach a scheme handled inside the webview, and having the agent debug
//! a *different* rendering of the artifact than the user is looking at is the
//! failure mode this whole feature exists to remove.
//!
//! What the page may reach is deliberately narrow. Scripts, styles and fonts
//! come from a fixed CDN allowlist; `connect-src 'none'` means no `fetch`, no
//! XHR, no WebSocket, no beacon; images are local or inline only. A remote
//! script host is still a low-bandwidth way for page content to leave the
//! machine (the request URL itself carries bytes) — that is the accepted cost of
//! letting artifacts use a CDN at all, and it is why the allowlist is one
//! constant rather than a wildcard.

use std::sync::OnceLock;

use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::db::Db;

/// The package CDNs an artifact may pull scripts, stylesheets and fonts from.
///
/// Pinned by host, never by wildcard. Every host here is a place a *published
/// package* is served from, which is the whole justification: a model asking
/// for React, D3, Tailwind or Chart.js reaches for one of these by reflex, and
/// before `ART-6` every one of those pages was blank. A host that serves
/// arbitrary user content does not belong on this list.
///
/// Each one is also a low-bandwidth way for page content to leave the machine —
/// the request URL itself carries bytes, whether or not the CDN 404s. That is
/// the accepted cost of letting artifacts use a CDN at all, and it is why this
/// is one constant you can read in ten seconds rather than a pattern.
const CDN_HOSTS: &str = "https://cdnjs.cloudflare.com \
                         https://cdn.jsdelivr.net \
                         https://unpkg.com \
                         https://esm.sh \
                         https://cdn.skypack.dev \
                         https://code.jquery.com \
                         https://cdn.tailwindcss.com \
                         https://d3js.org \
                         https://ga.jspm.io";

/// Google Fonts serves its stylesheets from here…
const STYLE_EXTRA: &str = "https://fonts.googleapis.com";
/// …and the font files themselves from here. The package CDNs are added to both
/// as well: icon fonts (Font Awesome, Material Symbols) ship as a stylesheet on
/// one and `woff2` files on the other.
const FONT_EXTRA: &str = "https://fonts.gstatic.com";

/// Console entries the in-page bridge keeps, matching the frontend panel's own
/// cap. A page in a render loop must not grow this without bound.
const CONSOLE_CAP: usize = 50;

/// Longest request head we will read. A preview request is one line and a few
/// headers; anything larger is not one of ours.
const MAX_REQUEST_HEAD: usize = 8 * 1024;

/// The base URL of the running preview server, including its token —
/// `http://127.0.0.1:<port>/<token>`.
///
/// Process-wide rather than threaded through `ToolContext`, for the same reason
/// `artifacts::CONSOLE` is: it is set once at startup, read from one toolset and
/// one command, and plumbing it through every `run_agent` caller would be a lot
/// of signature churn for a string that never changes.
static BASE: OnceLock<String> = OnceLock::new();

/// `http://127.0.0.1:<port>/<token>`, or `None` before the server has started.
pub fn base_url() -> Option<&'static str> {
    BASE.get().map(|s| s.as_str())
}

/// The URL that renders one artifact.
pub fn url_for(artifact_id: &str) -> Option<String> {
    base_url().map(|base| format!("{base}/{artifact_id}"))
}

/// The policy an artifact document is served under.
///
/// `'unsafe-inline'`/`'unsafe-eval'` are present because the document *is*
/// model-written inline script — refusing it would refuse every artifact — and
/// because `eval` buys an attacker nothing here that authoring the script
/// directly did not already buy them. The directives that matter are the ones
/// that say where bytes may go: `connect-src 'none'`, `form-action 'none'`, and
/// an `img-src` with no remote host.
fn csp() -> String {
    format!(
        "default-src 'none'; \
         script-src 'self' 'unsafe-inline' 'unsafe-eval' {CDN_HOSTS}; \
         style-src 'self' 'unsafe-inline' {STYLE_EXTRA} {CDN_HOSTS}; \
         font-src 'self' data: {FONT_EXTRA} {CDN_HOSTS}; \
         img-src 'self' data: blob:; \
         media-src 'self' data: blob:; \
         connect-src 'none'; \
         form-action 'none'; \
         base-uri 'none'; \
         frame-src 'none'"
    )
}

/// Injected ahead of the page's own scripts, in both surfaces.
///
/// It does two jobs at once, which is the point: the array is what
/// `check_preview` reads back out of headless Chrome, and the `postMessage` is
/// what the Canvas panel listens for. One capture, so the agent and the user
/// are looking at the same console rather than two that drift.
///
/// Kept small and defensive — it runs before anything the model wrote, so a bug
/// in here breaks every preview rather than one.
const BRIDGE: &str = r#"<script>(function(){
  var TAG="poiesis-preview-console";
  var LOG=window.__poiesis_console=[];
  var CAP=__CAP__;
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
  var FRAMED=window.parent!==window;
  function post(level,text,source){
    try{
      if(text.length>2000) text=text.slice(0,2000)+" …[truncated]";
      var entry={level:level,text:text,source:source||null};
      LOG.push(entry);
      if(LOG.length>CAP) LOG.splice(0,LOG.length-CAP);
      // Only when framed: in headless Chrome the page is the top document and
      // `parent` is itself, so this would post to nobody.
      if(FRAMED) parent.postMessage({tag:TAG,kind:"console",level:level,text:text,source:source||null},"*");
    }catch(e){}
  }
  // Announce this document to the panel, before its own `load` fires. The
  // panel counts these against the loads it sees: a document that arrives
  // without one is not ours, which is how a page that navigated itself away
  // gets noticed from outside (`ART-6`). Sent at parse time on purpose — a
  // page that redirects in its first script must still have said hello first.
  try{ if(FRAMED) parent.postMessage({tag:TAG,kind:"hello"},"*"); }catch(e){}
  ["log","info","warn","error","debug"].forEach(function(level){
    var original=console[level];
    console[level]=function(){
      var args=Array.prototype.slice.call(arguments);
      post(level,args.map(function(a){return render(a,0)}).join(" "));
      if(original) try{ original.apply(console,args); }catch(e){}
    };
  });
  // Capture phase, so this also sees a subresource failing to load — an <img>,
  // <script> or <link> that 404s fires `error` at the element, never at window,
  // and a missing script is the single most common reason a page is blank.
  window.addEventListener("error",function(e){
    var el=e.target;
    if(el&&el!==window&&el.tagName){
      var src=el.src||el.href||"";
      post("uncaught","failed to load "+el.tagName.toLowerCase()+(src?(" "+src):""));
      return;
    }
    var where=e.lineno?("line "+e.lineno+":"+(e.colno||0)):null;
    post("uncaught",(e.error&&(e.error.stack||e.error.message))||e.message||"script error",where);
  },true);
  window.addEventListener("unhandledrejection",function(e){
    post("uncaught","unhandled promise rejection: "+render(e.reason,0));
  });
  // The preview is served under a CSP the page did not write, so a blocked CDN
  // has to say so by name — otherwise "it just doesn't work" is all anyone gets.
  document.addEventListener("securitypolicyviolation",function(e){
    post("error","blocked by the preview's content policy: "+(e.blockedURI||"(inline)")+
      " violates "+e.violatedDirective+". Only these hosts are allowed: __ALLOWED__");
  });
  // A preview is a page, not a browser: following a link out of it would
  // replace the artifact with a website, inside a frame the user opened to look
  // at their own work. No CSP directive covers a document navigating itself, so
  // the two ways it happens on purpose are stopped here by hand, and the panel
  // catches the rest from outside by counting hellos.
  document.addEventListener("click",function(e){
    var el=e.target;
    var a=(el&&el.closest)?el.closest("a[href]"):null;
    if(!a) return;
    var to;
    try{ to=new URL(a.getAttribute("href"),location.href); }catch(err){ return; }
    if(to.origin===location.origin) return;
    e.preventDefault();
    post("error","blocked a link to "+to.href+" — a preview can't navigate away from itself. "+
      "Show the address as text if the user should be able to visit it.");
  },true);
  window.open=function(u){
    post("error","blocked window.open("+(u||"")+") — a preview can't open windows.");
    return null;
  };
})();</script>"#;

/// The bridge with its placeholders filled in.
fn bridge() -> String {
    BRIDGE
        .replace("__CAP__", &CONSOLE_CAP.to_string())
        .replace("__ALLOWED__", &format!("{CDN_HOSTS} {STYLE_EXTRA} {FONT_EXTRA}"))
}

/// Splice the bridge in ahead of everything the page brings with it.
///
/// After the doctype — a document whose first node is a `<script>` renders in
/// quirks mode, which would change the layout of the very page we are trying to
/// debug — and before any other script, or the hooks miss the errors that fire
/// during load, which are the ones that matter most.
pub fn document(content: &str) -> String {
    let bridge = bridge();
    match content.to_ascii_lowercase().find("<!doctype") {
        Some(at) => {
            // Splice after the doctype's closing `>`, wherever that lands.
            let rest = &content[at..];
            match rest.find('>') {
                Some(end) => {
                    let cut = at + end + 1;
                    format!("{}{}{}", &content[..cut], bridge, &content[cut..])
                }
                None => format!("{bridge}{content}"),
            }
        }
        None => format!("{bridge}{content}"),
    }
}

// ---- the server ----

/// Ids are generated by `db::new_id`, so anything outside this alphabet is not
/// an artifact id and is not worth a database round trip.
fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Split a request target into `(token, artifact_id)`, dropping any query
/// string. The query is how the frontend busts the cache when an artifact is
/// updated in place, so it carries no meaning here.
fn route(target: &str) -> Option<(&str, &str)> {
    let path = target.split(['?', '#']).next()?;
    let mut parts = path.trim_start_matches('/').split('/');
    let token = parts.next()?;
    let id = parts.next()?;
    if parts.next().is_some() || token.is_empty() || !is_id(id) {
        return None;
    }
    Some((token, id))
}

/// The request line's target, from a raw request head.
fn request_target(head: &str) -> Option<&str> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    parts.next()
}

fn response(status: &str, content_type: &str, extra: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         {extra}\
         Connection: close\r\n\r\n",
        body.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

/// Looks an artifact's html up by id. The one thing the server needs from the
/// rest of the app, behind a trait object so the socket can be tested without a
/// Tauri runtime or a database.
type Lookup = std::sync::Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Decide the whole response for one request head. Pure, so the two things that
/// keep this socket safe — the token check and the id check — are testable
/// together rather than one layer at a time.
fn respond(head: &str, token: &str, csp: &str, fetch: &Lookup) -> Vec<u8> {
    let body = request_target(head)
        .and_then(route)
        // The token is what stops any other process on this machine from
        // reading the user's artifacts off an open loopback port.
        .filter(|(t, _)| *t == token)
        .and_then(|(_, id)| fetch(id))
        .map(|content| document(&content));

    match body {
        Some(html) => response(
            "200 OK",
            "text/html; charset=utf-8",
            &format!("Content-Security-Policy: {csp}\r\n"),
            html.as_bytes(),
        ),
        None => response("404 Not Found", "text/plain; charset=utf-8", "", b"not found"),
    }
}

/// Serve one connection: read the request head, answer, close.
///
/// One request per connection (`Connection: close`), which is why reading up to
/// the blank line that ends the head is the whole of the protocol we speak.
async fn serve(mut stream: tokio::net::TcpStream, token: String, csp: String, fetch: Lookup) {
    let mut head = Vec::new();
    let mut buf = [0u8; 2048];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                head.extend_from_slice(&buf[..n]);
                if head.len() > MAX_REQUEST_HEAD {
                    return;
                }
                if let Ok(text) = std::str::from_utf8(&head) {
                    if text.contains("\r\n\r\n") {
                        let out = respond(text, &token, &csp, &fetch);
                        let _ = stream.write_all(&out).await;
                        let _ = stream.flush().await;
                        return;
                    }
                }
            }
            Err(_) => return,
        }
    }
}

/// Accept forever on an already-bound listener.
fn accept_loop(listener: TcpListener, token: String, csp: String, fetch: Lookup) {
    tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            tauri::async_runtime::spawn(serve(
                stream,
                token.clone(),
                csp.clone(),
                fetch.clone(),
            ));
        }
    });
}

/// Start the preview server. Binds synchronously so the port — and therefore
/// [`base_url`] — is known by the time `setup` returns and the frontend can ask
/// for it. Returns the base URL, or `None` if the loopback bind failed, in
/// which case previews fall back to the old inline rendering.
pub fn start(app: tauri::AppHandle) -> Option<String> {
    let token = uuid::Uuid::new_v4().simple().to_string();
    let listener =
        tauri::async_runtime::block_on(async { TcpListener::bind(("127.0.0.1", 0)).await })
            .map_err(|e| eprintln!("preview server: could not bind loopback: {e}"))
            .ok()?;
    let port = listener.local_addr().ok()?.port();
    let base = format!("http://127.0.0.1:{port}/{token}");

    // Only html is served. An svg or markdown artifact has no page to run, and
    // handing one out as `text/html` would be a way to smuggle markup in.
    let fetch: Lookup = std::sync::Arc::new(move |id: &str| {
        let db = app.try_state::<Db>()?;
        let artifact = db.get_artifact(id).ok().flatten()?;
        (artifact.kind == "html").then_some(artifact.content)
    });

    accept_loop(listener, token, csp(), fetch);
    let _ = BASE.set(base.clone());
    Some(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_lands_after_the_doctype_and_before_the_page() {
        let doc = document("<!DOCTYPE html>\n<script>boom()</script>");
        assert!(doc.starts_with("<!DOCTYPE html>"), "quirks mode would change the layout");
        let bridge_at = doc.find("__poiesis_console").unwrap();
        let page_at = doc.find("boom()").unwrap();
        assert!(bridge_at < page_at, "hooks must be installed before the page runs");
    }

    #[test]
    fn a_document_without_a_doctype_still_gets_the_bridge_first() {
        let doc = document("<h1>hi</h1>");
        assert!(doc.find("__poiesis_console").unwrap() < doc.find("<h1>").unwrap());
    }

    #[test]
    fn the_bridge_has_no_placeholders_left() {
        let doc = document("<p>x</p>");
        assert!(!doc.contains("__CAP__"));
        assert!(!doc.contains("__ALLOWED__"));
    }

    /// The panel's navigation guard works by counting these against the loads
    /// it sees, so a document that renders without sending one would be
    /// mistaken for a page that had navigated away — and put back, forever.
    #[test]
    fn every_served_document_announces_itself() {
        let doc = document("<p>x</p>");
        assert!(doc.contains(r#"kind:"hello""#));
        // Before the page's own code, so a script that redirects on line one
        // has still said hello first.
        assert!(doc.find(r#"kind:"hello""#).unwrap() < doc.find("<p>x</p>").unwrap());
    }

    /// The in-page half of the guard: the two ways a page leaves on purpose.
    #[test]
    fn links_out_and_window_open_are_refused_in_the_page() {
        let doc = document("<p>x</p>");
        assert!(doc.contains("blocked a link to "));
        assert!(doc.contains("window.open="));
    }

    /// A blocked host has to be named, with the alternatives — "it doesn't
    /// work" is what this whole feature exists to stop.
    #[test]
    fn a_blocked_host_is_reported_with_the_ones_that_would_work() {
        let doc = document("<p>x</p>");
        assert!(doc.contains("blocked by the preview's content policy"));
        assert!(doc.contains("https://unpkg.com"), "the allowlist is quoted to the page");
    }

    /// The whole point of the token: another process on this machine can reach
    /// the port, so the path is what it cannot guess.
    #[test]
    fn routing_needs_both_a_token_and_an_id() {
        assert_eq!(route("/tok/art-1"), Some(("tok", "art-1")));
        assert_eq!(route("/tok/art-1?v=99"), Some(("tok", "art-1")));
        assert_eq!(route("/tok/art-1#top"), Some(("tok", "art-1")));
        assert_eq!(route("/art-1"), None, "a bare id must not resolve");
        assert_eq!(route("/tok/art-1/extra"), None);
        assert_eq!(route("/"), None);
    }

    /// Path traversal and anything else that isn't an id is refused before the
    /// database is ever asked.
    #[test]
    fn only_id_shaped_segments_are_served() {
        assert!(is_id("abc123-_"));
        assert!(!is_id("../../etc/passwd"));
        assert!(!is_id("a b"));
        assert!(!is_id(""));
        assert_eq!(route("/tok/..%2f..%2fsecret"), None);
    }

    #[test]
    fn only_get_is_answered() {
        assert_eq!(request_target("GET /tok/a HTTP/1.1\r\nHost: x\r\n\r\n"), Some("/tok/a"));
        assert_eq!(request_target("POST /tok/a HTTP/1.1\r\n\r\n"), None);
        assert_eq!(request_target(""), None);
    }

    /// The directives that decide whether page content can leave the machine.
    /// `connect-src 'none'` is the one doing the work; if it ever loosens, the
    /// CDN allowlist stops being a narrow channel and becomes a wide one.
    #[test]
    fn the_policy_blocks_bulk_exfiltration() {
        let p = csp();
        assert!(p.contains("connect-src 'none'"), "no fetch/XHR/WebSocket/beacon");
        assert!(p.contains("form-action 'none'"), "no posting the page somewhere");
        assert!(!p.contains("img-src 'self' data: blob: http"), "no remote image pixels");
        assert!(p.contains("https://cdnjs.cloudflare.com"), "a CDN is the point of all this");
    }

    /// The libraries a model reaches for by reflex have to resolve, or the page
    /// is blank and nobody can see why. Scripts, stylesheets and fonts all come
    /// off the same package CDNs — a library whose JS loads but whose CSS is
    /// blocked renders as an unstyled mess, which is barely better than blank.
    #[test]
    fn the_usual_package_cdns_resolve_for_scripts_styles_and_fonts() {
        let p = csp();
        let script = p.split("script-src").nth(1).unwrap().split(';').next().unwrap();
        let style = p.split("style-src").nth(1).unwrap().split(';').next().unwrap();
        let font = p.split("font-src").nth(1).unwrap().split(';').next().unwrap();
        for host in [
            "https://cdnjs.cloudflare.com",
            "https://cdn.jsdelivr.net",
            "https://unpkg.com",
            "https://esm.sh",
            "https://cdn.tailwindcss.com",
            "https://code.jquery.com",
        ] {
            assert!(script.contains(host), "{host} must be loadable as a script");
            assert!(style.contains(host), "{host} must be loadable as a stylesheet");
            assert!(font.contains(host), "{host} must be loadable as a font");
        }
        assert!(style.contains("https://fonts.googleapis.com"));
        assert!(font.contains("https://fonts.gstatic.com"));
        // Tailwind's play CDN compiles at runtime; without this it throws.
        assert!(script.contains("'unsafe-eval'"));
    }

    /// Whatever else the allowlist grows, it must never grow a wildcard: the
    /// hosts are named because each one is a place bytes can leave for.
    #[test]
    fn the_allowlist_names_hosts_and_never_wildcards() {
        let p = csp();
        assert!(!p.contains('*'), "no wildcard host may appear in the policy");
        assert!(!p.contains("https: "), "no bare scheme source either");
    }

    fn fake_lookup() -> Lookup {
        std::sync::Arc::new(|id: &str| {
            (id == "art1").then(|| "<!doctype html><h1>hi</h1>".to_string())
        })
    }

    fn get(path: &str, token: &str) -> String {
        let head = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        String::from_utf8_lossy(&respond(&head, token, "test-policy", &fake_lookup())).into_owned()
    }

    /// The token is the only thing between an open loopback port and every
    /// artifact the user has. Guessing the id must not be enough.
    #[test]
    fn the_wrong_token_gets_nothing_even_with_the_right_id() {
        assert!(get("/wrong/art1", "right").starts_with("HTTP/1.1 404"));
        assert!(get("/art1", "right").starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn an_unknown_artifact_is_a_404_not_an_empty_page() {
        assert!(get("/right/nosuch", "right").starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn a_served_artifact_arrives_instrumented_and_under_its_own_policy() {
        let out = get("/right/art1?v=abc", "right");
        assert!(out.starts_with("HTTP/1.1 200 OK"));
        assert!(out.contains("Content-Security-Policy: test-policy"));
        assert!(out.contains("__poiesis_console"), "the bridge must be spliced in");
        assert!(out.contains("<h1>hi</h1>"));
    }

    /// End to end over a real socket — the parsing, the routing and the write
    /// path together, which is what the webview and Chrome each actually do.
    #[tokio::test]
    async fn a_real_request_over_the_loopback_socket_is_answered() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve(stream, "tok".into(), "test-policy".into(), fake_lookup()).await;
        });

        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        client
            .write_all(b"GET /tok/art1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .unwrap();
        let mut got = String::new();
        client.read_to_string(&mut got).await.unwrap();

        assert!(got.starts_with("HTTP/1.1 200 OK"), "got: {}", &got[..got.len().min(80)]);
        assert!(got.contains("Content-Security-Policy: test-policy"));
        assert!(got.contains("<h1>hi</h1>"));
    }

    #[test]
    fn a_response_carries_its_own_policy_and_no_cache() {
        let out = response("200 OK", "text/html", "Content-Security-Policy: x\r\n", b"hi");
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("Cache-Control: no-store\r\n"));
        assert!(text.contains("Content-Security-Policy: x\r\n"));
        assert!(text.ends_with("\r\n\r\nhi"));
    }
}
