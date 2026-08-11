//! Built-in Web Search toolset (TOOL-4). Privacy-first: the query is issued
//! directly from the user's machine to a **no-key** search endpoint (DuckDuckGo's
//! HTML site) — there is no Poiesis server in the middle and no provider account.
//! The top results (title, URL, snippet) plus the lead result's readable page
//! text are fed back to the model, with source attribution.
//!
//! Searching the web inherently means one network call leaves the device; the
//! toolset defaults off and the UI discloses this (§6.3 privacy posture).

use super::toolsets::{mark_untrusted, ToolContext};

const ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Poiesis/0.1";
/// How many search hits to summarize back to the model.
const MAX_RESULTS: usize = 6;
/// Cap on the lead page's extracted text, so a huge page can't blow the context.
const PAGE_TEXT_CAP: usize = 3000;
/// Cap for a directly-requested `fetch_url` read (LOOP-2): more headroom than a
/// search lead, since the user asked for this exact page.
const FETCH_URL_CAP: usize = 8000;

/// The OpenAI tool schemas advertised to the model for this toolset.
pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the public web and read the top results. Use for current events, facts, or anything not in the model's training. The search query leaves the device.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "The search query" }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "fetch_url",
                "description": "Fetch and read one web page the user referenced or a search result. The URL leaves this device.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "The full URL of the page to read" }
                    },
                    "required": ["url"]
                }
            }
        }
    ])
}

/// Is this a Web Search tool name?
pub fn handles(name: &str) -> bool {
    name == "web_search" || name == "fetch_url"
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense).
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "fetch_url" => {
            let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("a page");
            ("fetched".into(), url.to_string())
        }
        "web_search" => {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("the web");
            ("searched".into(), query.to_string())
        }
        other => {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("the web");
            (other.into(), query.to_string())
        }
    }
}

#[derive(Debug)]
struct SearchHit {
    title: String,
    url: String,
    snippet: String,
}

/// Execute a Web Search tool call: search, optionally read the lead page, and
/// return a model-readable digest with source attribution.
pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    if name == "fetch_url" {
        return fetch_url(ctx, args).await;
    }
    let query = args
        .get("query")
        .and_then(|q| q.as_str())
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or("missing 'query' argument")?;

    // Record the off-device action in the visible activity log (§6.1/§6.3).
    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "web", &format!("searched: {query}"));

    let hits = search(ctx.client, query).await?;
    if hits.is_empty() {
        return Ok(format!("No web results found for \"{query}\"."));
    }

    let mut body = String::new();
    for (i, h) in hits.iter().enumerate() {
        body.push_str(&format!("{}. {}\n   {}\n", i + 1, h.title, h.url));
        if !h.snippet.is_empty() {
            body.push_str(&format!("   {}\n", h.snippet));
        }
        body.push('\n');
    }

    // Best-effort: pull readable text from the lead result so the model has real
    // content, not just snippets. Failures here are non-fatal.
    if let Some(lead) = hits.first() {
        if let Ok(text) = fetch_readable(ctx.client, &lead.url, PAGE_TEXT_CAP).await {
            if !text.is_empty() {
                body.push_str(&format!("--- Content of {} ---\n{}\n", lead.url, text));
            }
        }
    }

    // TRU-2: one envelope for the whole digest rather than one per hit — DDG's
    // own titles/snippets are low-fidelity boilerplate, and marking each of up
    // to `MAX_RESULTS` separately would spend more of the context budget on
    // fence markers than on content. Any injection text anywhere in the digest
    // still ends up inside this one marked, scored region.
    let label = format!("web result: {}", domain_of(&hits[0].url));
    let wrapped = mark_untrusted(ctx, &label, &body);
    Ok(format!("Web results for \"{query}\":\n\n{wrapped}"))
}

/// Execute a `fetch_url` tool call (LOOP-2): read one page the model asked for,
/// capped for context safety. The URL leaves the device, so it is activity-logged.
async fn fetch_url(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let url = args
        .get("url")
        .and_then(|u| u.as_str())
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .ok_or("missing 'url' argument")?;

    let _ = ctx
        .db
        .log_activity(Some(ctx.conversation_id), "web", &format!("fetched {url}"));

    let text = fetch_readable(ctx.client, url, FETCH_URL_CAP).await?;
    if text.is_empty() {
        return Ok(format!("The page at {url} had no readable text."));
    }
    let label = format!("page at {}", domain_of(url));
    let wrapped = mark_untrusted(ctx, &label, &text);
    Ok(format!("--- Content of {url} ---\n{wrapped}"))
}

/// The host portion of a URL, for an untrusted-content label — `domain_of("https://example.com/a/b")`
/// is `"example.com"`.
fn domain_of(url: &str) -> String {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    without_scheme.split('/').next().unwrap_or(without_scheme).to_string()
}

/// Query DuckDuckGo's no-key HTML endpoint and parse the result list.
async fn search(client: &reqwest::Client, query: &str) -> Result<Vec<SearchHit>, String> {
    let resp = client
        .post(ENDPOINT)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .form(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("the search request failed: {e}"))?;
    let body = resp
        .text()
        .await
        .map_err(|e| format!("couldn't read the search response: {e}"))?;
    Ok(parse_results(&body))
}

/// Extract result rows from DuckDuckGo HTML by scanning for the stable
/// `result__a` (title/link) and `result__snippet` markers. Kept dependency-free:
/// a targeted scan rather than a full HTML parse.
fn parse_results(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let mut snippets = Vec::new();

    // Snippets first, in document order, to pair with titles positionally.
    for chunk in html.split("result__snippet").skip(1) {
        if let Some(text) = inner_text_after(chunk) {
            snippets.push(text);
        }
    }

    for chunk in html.split("result__a").skip(1) {
        let Some(href_raw) = attr_value(chunk, "href=\"") else { continue };
        let url = clean_url(&href_raw);
        if url.is_empty() {
            continue;
        }
        let title = inner_text_after(chunk).unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let snippet = snippets.get(hits.len()).cloned().unwrap_or_default();
        hits.push(SearchHit { title, url, snippet });
        if hits.len() >= MAX_RESULTS {
            break;
        }
    }
    hits
}

/// Read the value of an attribute like `href="..."` appearing soon after the
/// start of `chunk` (the marker we split on sits inside the same tag).
fn attr_value(chunk: &str, attr: &str) -> Option<String> {
    let start = chunk.find(attr)? + attr.len();
    let rest = &chunk[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// The text between the next `>` and its following `</a>`, tags stripped + HTML
/// entities decoded.
fn inner_text_after(chunk: &str) -> Option<String> {
    let gt = chunk.find('>')? + 1;
    let rest = &chunk[gt..];
    let end = rest.find("</a>").unwrap_or(rest.len().min(400));
    let raw = &rest[..end];
    let text = decode_entities(&strip_tags(raw));
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// DuckDuckGo wraps result links as `//duckduckgo.com/l/?uddg=<encoded>`; unwrap
/// to the real destination and normalize the scheme.
fn clean_url(href: &str) -> String {
    let href = decode_entities(href);
    if let Some(idx) = href.find("uddg=") {
        let after = &href[idx + 5..];
        let enc = after.split('&').next().unwrap_or(after);
        return percent_decode(enc);
    }
    if let Some(stripped) = href.strip_prefix("//") {
        return format!("https://{stripped}");
    }
    href
}

/// Fetch a page and reduce it to readable plain text (script/style removed, tags
/// stripped, whitespace collapsed, capped).
async fn fetch_readable(client: &reqwest::Client, url: &str, cap: usize) -> Result<String, String> {
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(html_to_text(&body, cap))
}

/// Crude readability: drop `<script>`/`<style>` blocks, strip remaining tags,
/// decode entities, and collapse whitespace. `cap` bounds the returned length.
fn html_to_text(html: &str, cap: usize) -> String {
    let without_blocks = remove_blocks(html, "script");
    let without_blocks = remove_blocks(&without_blocks, "style");
    let text = decode_entities(&strip_tags(&without_blocks));
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > cap {
        let mut end = cap;
        while !collapsed.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &collapsed[..end])
    } else {
        collapsed
    }
}

/// Remove `<tag ...>...</tag>` spans (used for script/style) case-insensitively.
fn remove_blocks(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0usize;
    while let Some(rel) = lower[cursor..].find(&open) {
        let start = cursor + rel;
        out.push_str(&html[cursor..start]);
        match lower[start..].find(&close) {
            Some(rel_end) => cursor = start + rel_end + close.len(),
            None => {
                cursor = html.len();
                break;
            }
        }
    }
    out.push_str(&html[cursor..]);
    out
}

/// Remove all `<...>` tags.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Decode the handful of HTML entities that show up in titles/snippets.
fn decode_entities(s: &str) -> String {
    let mut out = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    // Numeric decimal entities (e.g. &#8217;).
    while let Some(start) = out.find("&#") {
        let rest = &out[start + 2..];
        let Some(semi) = rest.find(';') else { break };
        let digits = &rest[..semi];
        let parsed = digits
            .strip_prefix(['x', 'X'])
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .or_else(|| digits.parse::<u32>().ok())
            .and_then(char::from_u32);
        match parsed {
            Some(ch) => {
                let mut replacement = String::new();
                replacement.push(ch);
                out.replace_range(start..start + 2 + semi + 1, &replacement);
            }
            None => break,
        }
    }
    out
}

/// Minimal percent-decoding for the unwrapped DuckDuckGo redirect URL.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_of_strips_scheme_and_path() {
        assert_eq!(domain_of("https://example.com/a/b?c=1"), "example.com");
        assert_eq!(domain_of("http://example.com"), "example.com");
        assert_eq!(domain_of("example.com/a"), "example.com");
    }

    #[test]
    fn parses_ddg_results_and_unwraps_links() {
        let html = r#"
          <div class="result">
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&amp;rut=abc">Example &amp; Title</a>
            <a class="result__snippet" href="x">A short <b>snippet</b> here.</a>
          </div>
        "#;
        let hits = parse_results(html);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://example.com/page");
        assert_eq!(hits[0].title, "Example & Title");
        assert_eq!(hits[0].snippet, "A short snippet here.");
    }

    #[test]
    fn html_to_text_drops_scripts_and_collapses() {
        let html = "<html><head><style>.x{color:red}</style></head><body>Hello <script>evil()</script> world</body></html>";
        assert_eq!(html_to_text(html, PAGE_TEXT_CAP), "Hello world");
    }
}
