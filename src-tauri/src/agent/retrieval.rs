//! `RET`: the `search_folder` tool — semantic search over whatever `IDX` has
//! read, sitting alongside `filesystem`'s exact-match `search_files`. Gated by
//! `Toolset::Indexing` (the same flag that gates building the index): with it
//! off there is nothing this tool could usefully search.
//!
//! Scoring is dot product (the chunks are pre-normalised, `IDX-4`/`VEC-2`)
//! plus a keyword bonus that can only add, then MMR diversification with a
//! per-file cap, then a floor below which nothing is returned at all
//! (`RET-2`). A weak top hit gets one corrective re-query with a rephrased
//! question (`RET-3`), and — if it's still weak — a one-shot sufficiency
//! check so a guess never reaches the model dressed as a confident answer
//! (`RET-4`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cloud::ChatEndpoint;
use crate::db::vectors::{similarity, ScopeSearch, VecHit};
use crate::db::SearchHit;
use crate::runtime::proxy::{CancelFlag, TurnOutcome};

use super::index::display_path;
use super::toolsets::{mark_untrusted, set_step_note, ToolContext};
use super::AgentEvent;

const DEFAULT_MAX_RESULTS: usize = 6;
const MAX_MAX_RESULTS: usize = 12;
/// Candidates pulled from the vector store before scoring narrows them down
/// (`RET-2`) — generous enough that MMR has real choices to diversify among.
const CANDIDATE_K: usize = 40;
const MMR_LAMBDA: f32 = 0.7;
const PER_FILE_CAP: usize = 2;
const SCORE_FLOOR: f32 = 0.40;
const CORRECTIVE_THRESHOLD: f32 = 0.50;
const SUFFICIENCY_THRESHOLD: f32 = 0.55;
/// `RRK-4`: rerank only the top candidates, and only when the ranking is
/// actually in doubt — the gap between the 1st and 5th score under this.
/// A confident retrieval skips the cross-encoder pass entirely.
const RERANK_GAP_THRESHOLD: f32 = 0.08;
/// How many of the top candidates get a cross-encoder pass when reranking
/// does run (`RRK-4`).
const RERANK_TOP_K: usize = 20;
/// Cap on the excerpt text handed back to the model — generous next to
/// `recall.rs`'s caps, because this *is* the grounding, not a pointer to more.
const RESULT_CAP: usize = 6000;
/// Per-excerpt clip within that cap, so one chunk can't eat the whole budget.
const EXCERPT_CAP: usize = 900;

/// Cap on `find_similar`'s results — a comparison tool, not a listing.
const FIND_SIMILAR_MAX: usize = 10;
/// Below this, two document centroids aren't worth mentioning as related at
/// all (`PHS-3`) — looser than `DOC_SIMILARITY_MIN` in `agent::duplicates`,
/// since this tool is "what's like this", not "what's a duplicate of this".
const FIND_SIMILAR_DOC_FLOOR: f32 = 0.5;

pub fn tool_specs() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "search_folder",
                "description": "Search the attached folder by meaning, for a question in your own words. Use search_files instead for a known filename, glob, or exact string. Only finds anything in a folder that has already been read (Workbench header, or Settings → Tools → Folder reading).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "A question or description, in plain words — not keywords." },
                        "path": { "type": "string", "description": "Optional: limit the search to this subfolder of the attached folder." },
                        "max_results": { "type": "integer", "description": "Cap on results (default 6)." }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "find_similar",
                "description": "Find files in the attached folder that look or read like a given one — near-duplicates, not just topically related. For an image, compares pixels. For a document, compares whole-file meaning over what's already been read (the folder needs to have been read first for that half).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file to compare against, inside the attached folder." }
                    },
                    "required": ["path"]
                }
            }
        }
    ])
}

pub fn handles(name: &str) -> bool {
    matches!(name, "search_folder" | "find_similar")
}

/// Human-readable (verb, target) for the timeline (§5.6 plain past-tense),
/// matching `search_files`'s own shape in `agent/filesystem.rs`.
pub fn describe(name: &str, args: &serde_json::Value) -> (String, String) {
    match name {
        "search_folder" => (
            "searched".into(),
            args.get("query").and_then(|q| q.as_str()).unwrap_or("the folder").to_string(),
        ),
        "find_similar" => (
            "compared".into(),
            args.get("path").and_then(|p| p.as_str()).unwrap_or("a file").to_string(),
        ),
        other => (other.into(), String::new()),
    }
}

pub async fn execute(
    ctx: &ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "search_folder" => search_folder(ctx, args).await,
        "find_similar" => find_similar(ctx, args).await,
        other => Err(format!("Folder reading doesn't handle '{other}'.")),
    }
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// Whether one more wrapped excerpt still fits under `RESULT_CAP` (`TRU-2`).
///
/// `search_folder` drops whole excerpts rather than letting `truncate` cut
/// through one. A byte cut mid-envelope strips the closing tag and the "this is
/// data" footer off the last excerpt, un-fencing exactly the text the wrapping
/// existed to fence. Six full-length excerpts overrun `RESULT_CAP` between them,
/// so this is the ordinary path and not an edge case. Kept free of a database
/// and an event sink so the budget is testable on its own, the same reason
/// `toolsets::check_render` is split out.
fn excerpt_fits(used: usize, prefix_len: usize, excerpt_len: usize, label: &str) -> bool {
    let projected =
        used + prefix_len + excerpt_len + crate::agent::untrusted::envelope_overhead(label);
    projected <= RESULT_CAP
}

fn truncate(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n(truncated)", &s[..cut])
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Common short/structural words a query might contain that carry no search
/// signal of their own — excluded from the keyword bonus even when they clear
/// the length bar (`RET-2`).
const STOPLIST: &[&str] = &[
    "the", "and", "for", "that", "this", "with", "from", "have", "what", "when", "where", "which",
    "about", "into", "your", "their", "there", "here", "were", "was", "are", "does", "did", "how",
    "who", "why", "can", "will", "would", "should", "could", "not", "any", "all", "our", "out",
];

/// A query term worth rewarding a chunk for containing, and how much
/// (`RET-2`'s "terms containing a digit or ≥4 chars, minus a stoplist; weight
/// 1.6 for terms with digits or ≥7 chars").
fn distinctive_terms(query: &str) -> Vec<(String, f32)> {
    query
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|t| !t.is_empty())
        .filter(|t| !STOPLIST.contains(&t.as_str()))
        .filter(|t| t.chars().any(|c| c.is_ascii_digit()) || t.chars().count() >= 4)
        .map(|t| {
            let weight = if t.chars().any(|c| c.is_ascii_digit()) || t.chars().count() >= 7 {
                1.6
            } else {
                1.0
            };
            (t, weight)
        })
        .collect()
}

/// The weighted fraction of `terms` present in `text` — never negative, so
/// adding `0.2 * keyword_bonus(...)` to a similarity score can only help.
fn keyword_bonus(terms: &[(String, f32)], text: &str) -> f32 {
    let total: f32 = terms.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let lower = text.to_lowercase();
    let present: f32 = terms.iter().filter(|(t, _)| lower.contains(t.as_str())).map(|(_, w)| w).sum();
    present / total
}

struct Scored {
    hit: VecHit,
    relevance: f32,
}

/// Greedy MMR (`RET-2`): each round picks the candidate maximising
/// `λ·relevance - (1-λ)·max_similarity_to_already_chosen`, skipping any file
/// that has already hit `per_file_cap`. Diversity is judged on the chunks'
/// own embeddings (`VecHit::vec`), not the combined relevance score.
fn mmr_select(mut candidates: Vec<Scored>, k: usize, lambda: f32, per_file_cap: usize) -> Vec<Scored> {
    let mut selected: Vec<Scored> = Vec::new();
    let mut file_counts: HashMap<String, usize> = HashMap::new();

    while selected.len() < k && !candidates.is_empty() {
        let mut best_pos = None;
        let mut best_mmr = f32::NEG_INFINITY;
        for (i, c) in candidates.iter().enumerate() {
            if *file_counts.get(&c.hit.ref_key).unwrap_or(&0) >= per_file_cap {
                continue;
            }
            let max_sim = selected
                .iter()
                .map(|s| similarity(&c.hit.vec, &s.hit.vec))
                .fold(0.0_f32, f32::max);
            let mmr = lambda * c.relevance - (1.0 - lambda) * max_sim;
            if mmr > best_mmr {
                best_mmr = mmr;
                best_pos = Some(i);
            }
        }
        let Some(pos) = best_pos else { break }; // everything left is over its file's cap
        let picked = candidates.remove(pos);
        *file_counts.entry(picked.hit.ref_key.clone()).or_insert(0) += 1;
        selected.push(picked);
    }
    selected
}

/// `RET-3`: one local-model call asking for the same question phrased "as a
/// document would state it". `None` on any failure — the caller just proceeds
/// with what it already had, exactly like `memory_skill::classify_scope`.
async fn rephrase_query(client: &reqwest::Client, endpoint: &ChatEndpoint, query: &str) -> Option<String> {
    let prompt = format!(
        "Rephrase this question the way a document would state the answer, keeping names and \
         numbers exactly as given. Reply with only the rephrased text, nothing else.\n\nQuestion: {query}"
    );
    let msgs = vec![serde_json::json!({ "role": "user", "content": prompt })];
    let outcome = crate::cloud::drive_turn(client, endpoint, &msgs, &[], 0.0, &CancelFlag::new(), |_| {})
        .await
        .ok()?;
    let TurnOutcome::Final { content } = outcome else { return None };
    let rephrased = content.trim();
    if rephrased.is_empty() {
        None
    } else {
        Some(rephrased.to_string())
    }
}

/// `RRK-4`: is the embedding-only ranking actually in doubt? Judged on the
/// gap between the 1st and 5th score — with fewer than 5 candidates the
/// lowest available stands in for the 5th, so a handful of closely-scored
/// candidates still counts as ambiguous.
fn ranking_in_doubt(scored: &[Scored]) -> bool {
    if scored.len() < 2 {
        return false; // nothing to disambiguate
    }
    let mut vals: Vec<f32> = scored.iter().map(|s| s.relevance).collect();
    vals.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let fifth = vals.get(4).copied().unwrap_or_else(|| *vals.last().unwrap());
    (vals[0] - fifth) < RERANK_GAP_THRESHOLD
}

/// `RRK-4`/`RRK-5`/`RRK-6`: when the ranking above is in doubt, send the top
/// candidates through the cross-encoder and let its score replace the hybrid
/// one **for those candidates only** — everything then re-sorts together in
/// `mmr_select`. Returns whether reranking actually ran, purely for
/// `RRK-UI-4`'s step-note suffix; any failure (no model installed, engine
/// down, a bad response) leaves `scored` exactly as it was — reranking must
/// never be able to fail a search (`RRK-5`).
async fn rerank_top(ctx: &ToolContext<'_>, query: &str, scored: &mut [Scored]) -> bool {
    if !ranking_in_doubt(scored) {
        return false;
    }
    let mut order: Vec<usize> = (0..scored.len()).collect();
    order.sort_by(|&a, &b| {
        scored[b].relevance.partial_cmp(&scored[a].relevance).unwrap_or(std::cmp::Ordering::Equal)
    });
    order.truncate(RERANK_TOP_K);

    let documents: Vec<String> = order.iter().map(|&i| clip(&scored[i].hit.text, EXCERPT_CAP)).collect();
    match crate::commands::rerankgen::rerank_or_none(ctx.mgr, ctx.rerank_mgr, ctx.db, query, &documents).await {
        Some(scores) if scores.len() == order.len() => {
            for (&i, score) in order.iter().zip(scores) {
                scored[i].relevance = score;
            }
            true
        }
        _ => false,
    }
}

/// `RET-4`: one local-model call asking whether the closest excerpts actually
/// answer the question, only spent when the top score is already weak.
/// `None` (can't tell — no local model, or the call failed) is treated as
/// "no" by the caller: a weak score we can't corroborate stays a warning.
async fn judges_sufficient(
    client: &reqwest::Client,
    endpoint: &ChatEndpoint,
    query: &str,
    excerpts: &str,
) -> Option<bool> {
    let prompt = format!(
        "A user asked: \"{query}\"\n\nHere are the closest excerpts found in their files:\n{excerpts}\n\n\
         Do these excerpts actually answer the question, or do they only share its general subject \
         without answering it? Answer with exactly one word: yes or no."
    );
    let msgs = vec![serde_json::json!({ "role": "user", "content": prompt })];
    let outcome = crate::cloud::drive_turn(client, endpoint, &msgs, &[], 0.0, &CancelFlag::new(), |_| {})
        .await
        .ok()?;
    let TurnOutcome::Final { content } = outcome else { return None };
    let lower = content.to_lowercase();
    if lower.contains("yes") {
        Some(true)
    } else if lower.contains("no") {
        Some(false)
    } else {
        None
    }
}

async fn search_folder(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(|q| q.as_str())
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or("missing 'query' argument")?;
    let max_results = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .clamp(1, MAX_MAX_RESULTS);

    let (folder, _trust) = ctx.db.conversation_folder(ctx.conversation_id).map_err(|e| e.to_string())?;
    let Some(folder) = folder else {
        return Err("No folder is attached to this conversation.".into());
    };
    let root = crate::permissions::canonicalize_lenient(Path::new(&folder));
    let scope_key = root.to_string_lossy().to_string();
    let folder_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| scope_key.clone());

    // RET-1's optional `path`: narrow the search to one subfolder of the
    // attached root, rather than a second, independently-granted scope — the
    // index is built (and trusted) one root at a time.
    let subfolder = args.get("path").and_then(|p| p.as_str()).map(str::trim).filter(|p| !p.is_empty());
    let scope_prefix = subfolder.map(|p| {
        let joined = if Path::new(p).is_relative() { root.join(p) } else { PathBuf::from(p) };
        crate::permissions::canonicalize_lenient(&joined)
    });
    let in_scope = |ref_key: &str| -> bool {
        scope_prefix.as_ref().map(|p| Path::new(ref_key).starts_with(p)).unwrap_or(true)
    };

    let Some(index_root) = ctx.db.get_index_root(&scope_key).map_err(|e| e.to_string())? else {
        return Ok(format!(
            "I haven't read {folder_name} yet, so I can't search it by meaning — ask to have it read first."
        ));
    };
    if index_root.file_count == 0 {
        return Ok(format!("I've read {folder_name}, but found nothing searchable in it."));
    }

    // RET-UI-3: an honest "N files changed" note piggybacks on the same
    // stat-only walk IDX-UI-3 already uses, only paid for on a stale root.
    let stale_note = if index_root.state == "stale" {
        let changed = super::index::count_changed(ctx.db, &root, &scope_key);
        if changed > 0 {
            format!(" — {} changed since I read it", plural(changed, "file"))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let Some((mut vectors, model_name, dim)) =
        crate::commands::embedgen::embed_texts_or_none(ctx.mgr, ctx.embed_mgr, ctx.db, &[query.to_string()]).await
    else {
        return Ok(format!(
            "I can't search {folder_name} by meaning right now — the recall engine isn't ready."
        ));
    };
    let query_vec = vectors.pop().unwrap_or_default();

    let raw_hits = match ctx
        .db
        .search_vectors("file", &scope_key, &model_name, dim, &query_vec, CANDIDATE_K)
        .map_err(|e| e.to_string())?
    {
        ScopeSearch::Stale => {
            return Ok(format!(
                "{folder_name} needs to be read again before I can search it — the way I read files has changed."
            ));
        }
        ScopeSearch::Hits(hits) => hits,
    };

    let terms = distinctive_terms(query);
    let mut scored: Vec<Scored> = raw_hits
        .into_iter()
        .filter(|hit| in_scope(&hit.ref_key))
        .map(|hit| {
            let kw = keyword_bonus(&terms, &hit.text);
            let relevance = (hit.score + 0.2 * kw).min(1.0);
            Scored { hit, relevance }
        })
        .filter(|s| s.relevance >= SCORE_FLOOR)
        .collect();

    let mut best = scored.iter().map(|s| s.relevance).fold(0.0_f32, f32::max);

    // RET-3: a weak best hit gets one corrective re-query, not a retry loop.
    if best < CORRECTIVE_THRESHOLD {
        if let Some(endpoint) = ctx.local_endpoint {
            if let Some(rephrased) = rephrase_query(ctx.client, endpoint, query).await {
                if let Some((mut rvecs, rmodel, rdim)) =
                    crate::commands::embedgen::embed_texts_or_none(ctx.mgr, ctx.embed_mgr, ctx.db, &[rephrased])
                        .await
                {
                    // The scope may have gone stale mid-call; only merge a
                    // re-query answered in the same embedding space.
                    if rmodel == model_name && rdim == dim {
                        let rvec = rvecs.pop().unwrap_or_default();
                        if let Ok(ScopeSearch::Hits(more)) =
                            ctx.db.search_vectors("file", &scope_key, &rmodel, rdim, &rvec, CANDIDATE_K)
                        {
                            let mut seen: std::collections::HashSet<(String, i64)> =
                                scored.iter().map(|s| (s.hit.ref_key.clone(), s.hit.chunk_ix)).collect();
                            for hit in more {
                                if !in_scope(&hit.ref_key) {
                                    continue;
                                }
                                let key = (hit.ref_key.clone(), hit.chunk_ix);
                                if !seen.insert(key) {
                                    continue;
                                }
                                let kw = keyword_bonus(&terms, &hit.text);
                                let relevance = (hit.score + 0.2 * kw).min(1.0);
                                if relevance >= SCORE_FLOOR {
                                    best = best.max(relevance);
                                    scored.push(Scored { hit, relevance });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // RRK-4/5/6: a second pass over the top candidates when the ranking above
    // is still in doubt. Off by default (RRK, RRK-UI-2) — `rerank_or_none`
    // reads the setting itself and is a no-op until installed and turned on.
    let reranked = rerank_top(ctx, query, &mut scored).await;
    if reranked {
        best = scored.iter().map(|s| s.relevance).fold(0.0_f32, f32::max);
    }

    let final_hits = mmr_select(scored, max_results, MMR_LAMBDA, PER_FILE_CAP);

    if final_hits.is_empty() {
        let _ = ctx.db.log_activity(
            Some(ctx.conversation_id),
            "file",
            &format!("searched {folder_name} for \"{query}\" (0 hits)"),
        );
        set_step_note(ctx, format!("— nothing in {folder_name}{stale_note}"));
        return Ok(format!("I couldn't find anything about that in {folder_name}{stale_note}."));
    }

    // RET-4: still weak after the corrective pass — ask once whether the
    // closest excerpts actually answer the question rather than just share
    // its subject. Can't corroborate ⇒ treat the weak score as the warning.
    let mut warn = false;
    if best < SUFFICIENCY_THRESHOLD {
        let sample = final_hits
            .iter()
            .take(3)
            .map(|s| clip(&s.hit.text, EXCERPT_CAP))
            .collect::<Vec<_>>()
            .join("\n---\n");
        let sufficient = match ctx.local_endpoint {
            Some(endpoint) => judges_sufficient(ctx.client, endpoint, query, &sample).await,
            None => None,
        };
        warn = sufficient != Some(true);
    }

    let matches: Vec<SearchHit> = final_hits
        .iter()
        .map(|s| {
            let path = Path::new(&s.hit.ref_key);
            let title = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| s.hit.ref_key.clone());
            let created_at = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            SearchHit {
                source: "file".to_string(),
                conversation_id: None,
                title,
                created_at,
                snippet: clip(&s.hit.text, 220),
                kind: None,
                path: Some(s.hit.ref_key.clone()),
            }
        })
        .collect();

    ctx.sink.emit(AgentEvent::Recall { id: ctx.call_id.to_string(), matches: matches.clone() });

    let n_files = final_hits
        .iter()
        .map(|s| s.hit.ref_key.clone())
        .collect::<std::collections::HashSet<_>>()
        .len();
    let _ = ctx.db.log_activity(
        Some(ctx.conversation_id),
        "file",
        &format!("searched {folder_name} for \"{query}\" ({} hits in {})", final_hits.len(), plural(n_files, "file")),
    );

    // RET-UI-2: the step line *is* the grounding statement, and a weak
    // retrieval has to be legible as one without reading the answer critically.
    // RRK-UI-4: reranking earns a quiet suffix; skipping it says nothing at
    // all — silence is the correct signal that the ranking was already clear.
    let rerank_suffix = if reranked { " — re-read the closest ones" } else { "" };
    set_step_note(
        ctx,
        if warn {
            format!("— I'm not sure they answer this{rerank_suffix}")
        } else {
            format!("— {} in {folder_name}{rerank_suffix}", plural(n_files, "file"))
        },
    );

    let mut out = String::new();
    if warn {
        out.push_str(
            "Note: these excerpts may only share the subject of the question, not answer it — \
             don't present them as a confident answer unless they clearly do.\n\n",
        );
    }
    out.push_str(&format!("Found in {folder_name}:\n"));
    for (i, s) in final_hits.iter().enumerate() {
        let rel = display_path(Path::new(&s.hit.ref_key), &root);
        // TRU-2: each excerpt is marked with the file it actually came from —
        // unlike a web digest, these are genuinely separate outside sources
        // mixed together, and the label is exactly which one said what.
        let label = format!("file {rel}");
        let excerpt = clip(&s.hit.text, EXCERPT_CAP);
        let prefix = format!("{}. [{rel}] \n", i + 1);
        if !excerpt_fits(out.len(), prefix.len(), excerpt.len(), &label) {
            out.push_str(&format!("({} more not shown here)\n", final_hits.len() - i));
            break;
        }
        let wrapped = mark_untrusted(ctx, &label, &excerpt);
        out.push_str(&format!("{}. [{rel}] {wrapped}\n", i + 1));
    }
    Ok(truncate(out, RESULT_CAP))
}

/// `PHS-3`'s tool half: what else in the attached folder looks or reads like
/// one given file. Branches on the target's own extension rather than a mode
/// argument — the model just names a file, the same way `search_files` does.
async fn find_similar(ctx: &ToolContext<'_>, args: &serde_json::Value) -> Result<String, String> {
    let path_arg = args
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or("missing 'path' argument")?;

    let (folder, _trust) = ctx.db.conversation_folder(ctx.conversation_id).map_err(|e| e.to_string())?;
    let Some(folder) = folder else {
        return Err("No folder is attached to this conversation.".into());
    };
    let root = crate::permissions::canonicalize_lenient(Path::new(&folder));
    let scope_key = root.to_string_lossy().to_string();
    let folder_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| scope_key.clone());

    let target = crate::permissions::canonicalize_lenient(&{
        let p = Path::new(path_arg);
        if p.is_relative() { root.join(p) } else { p.to_path_buf() }
    });
    // The comparison only ever runs inside the attached folder, and so must
    // its subject: an absolute path outside the root would have this tool read
    // bytes the conversation was never granted.
    if !crate::permissions::path_within_root(&target, &root) {
        return Err(format!("{path_arg} is outside {folder_name}, so I can't look at it."));
    }
    if !target.is_file() {
        return Err(format!("{path_arg} isn't a file I can compare."));
    }

    if super::index::has_ext(&target, &super::index::IMAGE_EXTS) {
        // Through the same path+mtime cache the folder scan fills (PHS-1), so
        // asking twice doesn't re-decode the whole folder.
        let Some(target_hash) = super::duplicates::cached_or_computed_hash(ctx.db, &target) else {
            return Err(format!("{path_arg} doesn't look like a readable image."));
        };
        let mut hits: Vec<(PathBuf, u32)> = super::index::walk_files(&root)
            .into_iter()
            .filter(|p| super::index::has_ext(p, &super::index::IMAGE_EXTS) && p != &target)
            .filter_map(|p| {
                super::duplicates::cached_or_computed_hash(ctx.db, &p)
                    .map(|h| (p, super::phash::hamming(target_hash, h)))
            })
            .filter(|(_, d)| *d <= super::phash::RELATED_MAX)
            .collect();
        hits.sort_by_key(|(_, d)| *d);
        hits.truncate(FIND_SIMILAR_MAX);

        if hits.is_empty() {
            return Ok(format!("Nothing else in {folder_name} looks like {path_arg}."));
        }
        let mut out = format!("Images in {folder_name} that look like {path_arg}:\n");
        for (p, d) in &hits {
            let rel = display_path(p, &root);
            let label = if *d <= super::phash::IDENTICAL_MAX {
                "identical"
            } else if *d <= super::phash::NEAR_MAX {
                "near-duplicate"
            } else {
                "visually related"
            };
            out.push_str(&format!("- [{rel}] {label} (distance {d})\n"));
        }
        return Ok(out);
    }

    // Document half: compare against whatever IDX has already embedded — no
    // reading or embedding happens here.
    let ref_key = target.to_string_lossy().to_string();
    let centroids = ctx.db.file_centroids(&scope_key).map_err(|e| e.to_string())?;
    let Some(target_vec) = centroids.iter().find(|(k, _)| *k == ref_key).map(|(_, v)| v.clone()) else {
        return Ok(format!(
            "{path_arg} hasn't been read yet — read {folder_name} first so I have something to compare it to."
        ));
    };

    let mut hits: Vec<(String, f32)> = centroids
        .iter()
        .filter(|(k, _)| *k != ref_key)
        .map(|(k, v)| (k.clone(), similarity(&target_vec, v)))
        .filter(|(_, s)| *s >= FIND_SIMILAR_DOC_FLOOR)
        .collect();
    hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(FIND_SIMILAR_MAX);

    if hits.is_empty() {
        return Ok(format!("Nothing else in {folder_name} reads like {path_arg}."));
    }
    let mut out = format!("Documents in {folder_name} that read like {path_arg}:\n");
    for (k, s) in &hits {
        let rel = display_path(Path::new(k), &root);
        let label = if *s >= 0.93 { "near-duplicate" } else { "related" };
        out.push_str(&format!("- [{rel}] {label} ({:.0}% similar)\n", s * 100.0));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(ref_key: &str, chunk_ix: i64, text: &str, score: f32, vec: Vec<f32>) -> VecHit {
        VecHit { ref_key: ref_key.to_string(), chunk_ix, text: text.to_string(), score, vec }
    }

    /// `TRU-2`: the reason `excerpt_fits` exists. A run of full-length excerpts
    /// overruns `RESULT_CAP` before the default result count is reached, so the
    /// budget has to stop on a whole-envelope boundary — the old unconditional
    /// `truncate` cut the last envelope open instead.
    #[test]
    fn the_excerpt_budget_stops_before_it_would_cut_an_envelope() {
        let label = "file notes/some-long-file-name.md";
        let prefix = "1. [notes/some-long-file-name.md] \n".len();
        let mut used = "Found in notes:\n".len();
        let mut accepted = 0;
        for _ in 0..DEFAULT_MAX_RESULTS {
            if !excerpt_fits(used, prefix, EXCERPT_CAP, label) {
                break;
            }
            used += prefix + EXCERPT_CAP + crate::agent::untrusted::envelope_overhead(label);
            accepted += 1;
        }
        assert!(
            accepted < DEFAULT_MAX_RESULTS,
            "full-length excerpts should exhaust the budget before {DEFAULT_MAX_RESULTS} of them fit"
        );
        assert!(accepted > 0, "at least one excerpt must always fit");
        assert!(used <= RESULT_CAP, "what was accepted stays whole under the cap");
    }

    #[test]
    fn distinctive_terms_drop_short_stopwords_and_keep_the_rest() {
        let terms = distinctive_terms("what is the Q3 budget for marketing");
        let words: Vec<&str> = terms.iter().map(|(t, _)| t.as_str()).collect();
        assert!(!words.contains(&"what"), "3-letter stopword-adjacent word should drop");
        assert!(!words.contains(&"the"));
        assert!(!words.contains(&"for"));
        assert!(words.contains(&"q3"), "a short term with a digit still counts");
        assert!(words.contains(&"budget"));
        assert!(words.contains(&"marketing"));
    }

    #[test]
    fn terms_with_a_digit_or_seven_plus_chars_weigh_more() {
        let terms = distinctive_terms("q3 marketing plan");
        let by_term: HashMap<&str, f32> = terms.iter().map(|(t, w)| (t.as_str(), *w)).collect();
        assert_eq!(by_term["q3"], 1.6, "has a digit");
        assert_eq!(by_term["marketing"], 1.6, ">= 7 chars");
        assert_eq!(by_term["plan"], 1.0, "plain 4-char term");
    }

    #[test]
    fn keyword_bonus_is_the_weighted_fraction_present() {
        let terms = distinctive_terms("q3 marketing plan");
        // Only "marketing" (weight 1.6 of a 1.6+1.6+1.0=4.2 total) appears.
        let bonus = keyword_bonus(&terms, "the new marketing brochure");
        assert!((bonus - (1.6 / 4.2)).abs() < 1e-4);
    }

    #[test]
    fn keyword_bonus_is_zero_for_no_distinctive_terms() {
        // A query of nothing but stopwords yields no terms to score against.
        assert_eq!(keyword_bonus(&distinctive_terms("what is this for"), "anything at all"), 0.0);
    }

    #[test]
    fn mmr_prefers_the_higher_scoring_candidate_when_tied_on_diversity() {
        let candidates = vec![
            Scored { hit: hit("a.md", 0, "a", 0.9, vec![1.0, 0.0]), relevance: 0.9 },
            Scored { hit: hit("b.md", 0, "b", 0.5, vec![0.0, 1.0]), relevance: 0.5 },
        ];
        let picked = mmr_select(candidates, 2, 0.7, 2);
        assert_eq!(picked[0].hit.ref_key, "a.md", "higher relevance goes first");
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn mmr_penalises_a_near_duplicate_of_an_already_selected_chunk() {
        // Two near-identical chunks from the same near-duplicate content vs.
        // one lower-scoring but genuinely different chunk — MMR should still
        // fit the different one in ahead of the redundant near-duplicate once
        // the first has already been taken, given enough of a diversity gap.
        let candidates = vec![
            Scored { hit: hit("a.md", 0, "a", 0.95, vec![1.0, 0.0]), relevance: 0.95 },
            Scored { hit: hit("a.md", 1, "a-dup", 0.94, vec![0.99, 0.14]), relevance: 0.94 },
            Scored { hit: hit("b.md", 0, "b", 0.60, vec![0.0, 1.0]), relevance: 0.60 },
        ];
        let picked = mmr_select(candidates, 3, 0.5, 2);
        // With the per-file cap at 2, both a.md chunks *could* be picked, but
        // the near-duplicate's similarity penalty should push b.md ahead of
        // it in the ranking.
        assert_eq!(picked[0].hit.ref_key, "a.md");
        assert_eq!(picked[1].hit.ref_key, "b.md", "the diverse chunk should outrank the near-duplicate");
    }

    #[test]
    fn mmr_respects_the_per_file_cap() {
        let candidates = vec![
            Scored { hit: hit("a.md", 0, "a0", 0.9, vec![1.0, 0.0]), relevance: 0.9 },
            Scored { hit: hit("a.md", 1, "a1", 0.8, vec![0.9, 0.1]), relevance: 0.8 },
            Scored { hit: hit("a.md", 2, "a2", 0.7, vec![0.8, 0.2]), relevance: 0.7 },
        ];
        let picked = mmr_select(candidates, 3, 0.7, 2);
        assert_eq!(picked.len(), 2, "a third chunk from the same file must not be selected");
    }

    #[test]
    fn floor_excludes_low_relevance_hits_before_selection() {
        let scored: Vec<Scored> = vec![
            Scored { hit: hit("a.md", 0, "a", 0.5, vec![1.0]), relevance: 0.5 },
            Scored { hit: hit("b.md", 0, "b", 0.2, vec![1.0]), relevance: 0.2 },
        ]
        .into_iter()
        .filter(|s| s.relevance >= SCORE_FLOOR)
        .collect();
        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].hit.ref_key, "a.md");
    }

    #[test]
    fn ranking_in_doubt_when_the_top_scores_are_close() {
        let scored = vec![
            Scored { hit: hit("a.md", 0, "a", 0.90, vec![]), relevance: 0.90 },
            Scored { hit: hit("b.md", 0, "b", 0.89, vec![]), relevance: 0.89 },
            Scored { hit: hit("c.md", 0, "c", 0.88, vec![]), relevance: 0.88 },
        ];
        assert!(ranking_in_doubt(&scored), "a tight cluster with no 5th hit is still ambiguous");
    }

    #[test]
    fn ranking_not_in_doubt_when_the_top_hit_clearly_leads() {
        let scored = vec![
            Scored { hit: hit("a.md", 0, "a", 0.95, vec![]), relevance: 0.95 },
            Scored { hit: hit("b.md", 0, "b", 0.50, vec![]), relevance: 0.50 },
            Scored { hit: hit("c.md", 0, "c", 0.45, vec![]), relevance: 0.45 },
            Scored { hit: hit("d.md", 0, "d", 0.44, vec![]), relevance: 0.44 },
            Scored { hit: hit("e.md", 0, "e", 0.43, vec![]), relevance: 0.43 },
        ];
        assert!(!ranking_in_doubt(&scored), "a clear leader over 5 candidates needs no second look");
    }

    #[test]
    fn ranking_never_in_doubt_with_a_single_candidate() {
        let scored = vec![Scored { hit: hit("a.md", 0, "a", 0.90, vec![]), relevance: 0.90 }];
        assert!(!ranking_in_doubt(&scored), "nothing to disambiguate against");
    }
}
