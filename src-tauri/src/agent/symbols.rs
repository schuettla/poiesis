//! Code-shaped navigation (`COD-14`..`COD-16`): the top-level symbols of a
//! file, chunks that follow them, and `find_symbol`.
//!
//! Parsing is tree-sitter with each grammar's own `tags.scm`, the query the
//! grammar authors ship for exactly this ("which nodes are definitions, and what
//! is their name"). JSON and Markdown have no tags query; their symbols are the
//! top-level keys and the headings. **No LSP**: see the plan.
//!
//! Symbols are parsed on demand and cached per file by mtime and size, not
//! stored at index time. `find_symbol` then works in a folder that was never
//! indexed, and with no recall model installed at all.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

use super::filesystem::looks_binary;

/// Files larger than this are searched for references as text but not parsed.
const MAX_PARSE_BYTES: u64 = 1024 * 1024;
/// Entries past this empty the cache rather than grow it without bound.
const MAX_CACHED_FILES: usize = 5000;
const MAX_CONTEXT_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
    Json,
    Markdown,
}

/// What a grammar's tags query leaves out that a reader still looks for.
const RUST_EXTRA: &str = r#"
(const_item name: (identifier) @name) @definition.constant
(static_item name: (identifier) @name) @definition.constant
"#;
const TS_EXTRA: &str = r#"
(type_alias_declaration name: (type_identifier) @name) @definition.type
(enum_declaration name: (identifier) @name) @definition.enum
"#;

impl Lang {
    pub fn for_path(path: &Path) -> Option<Lang> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "rs" => Lang::Rust,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
            "py" => Lang::Python,
            "go" => Lang::Go,
            "json" => Lang::Json,
            "md" | "markdown" => Lang::Markdown,
            _ => return None,
        })
    }

    /// Code, as opposed to data and prose: only code chunks on symbols.
    pub fn is_code(self) -> bool {
        !matches!(self, Lang::Json | Lang::Markdown)
    }

    fn language(self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Json => tree_sitter_json::LANGUAGE.into(),
            Lang::Markdown => tree_sitter_md::LANGUAGE.into(),
        }
    }

    /// The tags query source. TypeScript's own file only adds to JavaScript's,
    /// the way tree-sitter's tagger combines them.
    fn tags_source(self) -> Option<String> {
        Some(match self {
            Lang::Rust => format!("{}\n{RUST_EXTRA}", tree_sitter_rust::TAGS_QUERY),
            Lang::TypeScript | Lang::Tsx => format!(
                "{}\n{}\n{TS_EXTRA}",
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            ),
            Lang::JavaScript => tree_sitter_javascript::TAGS_QUERY.to_string(),
            Lang::Python => tree_sitter_python::TAGS_QUERY.to_string(),
            Lang::Go => tree_sitter_go::TAGS_QUERY.to_string(),
            Lang::Json | Lang::Markdown => return None,
        })
    }
}

/// One definition in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// `function`, `method`, `class`, `interface`, `module`, `type`, `constant`,
    /// `key`, `heading`, ... as the tags query names it.
    pub kind: String,
    pub bytes: Range<usize>,
    /// 1-based.
    pub line: usize,
    pub end_line: usize,
}

/// Compiled once per language. A query that fails to compile against its own
/// grammar leaves that language with no symbols rather than failing the app.
fn query(lang: Lang) -> Option<&'static Query> {
    static QUERIES: OnceLock<HashMap<Lang, Option<Query>>> = OnceLock::new();
    QUERIES
        .get_or_init(|| {
            [Lang::Rust, Lang::TypeScript, Lang::Tsx, Lang::JavaScript, Lang::Python, Lang::Go]
                .into_iter()
                .map(|l| (l, l.tags_source().and_then(|src| Query::new(&l.language(), &src).ok())))
                .collect()
        })
        .get(&lang)
        .and_then(|q| q.as_ref())
}

fn symbol(name: &str, kind: &str, node: Node) -> Symbol {
    Symbol {
        name: name.to_string(),
        kind: kind.to_string(),
        bytes: node.byte_range(),
        line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    }
}

/// `COD-14`: every definition in `text`, in file order. Nested ones (a method
/// in an `impl` or a class) are included; `top_level` filters them out.
pub fn extract(lang: Lang, text: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    if parser.set_language(&lang.language()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else { return Vec::new() };
    let src = text.as_bytes();
    let root = tree.root_node();
    let mut out = Vec::new();

    match lang {
        Lang::Json => {
            if let Some(object) = root.named_child(0).filter(|n| n.kind() == "object") {
                let mut cursor = object.walk();
                for pair in object.named_children(&mut cursor).filter(|n| n.kind() == "pair") {
                    if let Some(key) = pair.child_by_field_name("key").and_then(|k| k.utf8_text(src).ok()) {
                        out.push(symbol(key.trim_matches('"'), "key", pair));
                    }
                }
            }
        }
        Lang::Markdown => collect_headings(root, src, &mut out),
        _ => {
            let Some(query) = query(lang) else { return Vec::new() };
            let names = query.capture_names();
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(query, root, src);
            while let Some(m) = matches.next() {
                let mut name = None;
                let mut def = None;
                for c in m.captures {
                    let cap = names[c.index as usize];
                    if cap == "name" {
                        name = c.node.utf8_text(src).ok();
                    } else if let Some(kind) = cap.strip_prefix("definition.") {
                        def = Some((kind, c.node));
                    }
                }
                if let (Some(name), Some((kind, node))) = (name, def) {
                    out.push(symbol(name, kind, node));
                }
            }
        }
    }

    // One node can match two patterns (a Rust method is also a function); the
    // first, more specific pattern wins.
    out.sort_by_key(|s| s.bytes.start);
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert((s.bytes.start, s.bytes.end)));
    out
}

fn collect_headings(node: Node, src: &[u8], out: &mut Vec<Symbol>) {
    if matches!(node.kind(), "atx_heading" | "setext_heading") {
        if let Ok(text) = node.utf8_text(src) {
            let title = text.lines().next().unwrap_or("").trim().trim_start_matches('#').trim();
            if !title.is_empty() {
                out.push(symbol(title, "heading", node));
            }
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_headings(child, src, out);
    }
}

/// Definitions not inside another definition.
pub fn top_level(symbols: &[Symbol]) -> Vec<Symbol> {
    let mut out: Vec<Symbol> = Vec::new();
    for s in symbols {
        if out.last().map_or(true, |prev| s.bytes.start >= prev.bytes.end) {
            out.push(s.clone());
        }
    }
    out
}

// ---- COD-15: chunks on symbol boundaries ----

/// Chunks of a code file that end on symbol boundaries, each headed with the
/// path and the symbols it holds. `None` when the file is not code in a known
/// grammar, or has no symbols: the prose chunker takes it then.
///
/// Each top-level symbol takes the text before it (its doc comment,
/// attributes, imports) with it, so the pieces cover the whole file. Pieces
/// are packed up to `budget` bytes without splitting one. A single symbol past
/// `max_symbol` bytes is the one exception: it is cut on line boundaries, each
/// part saying which part it is, because an embedding request has a limit too.
pub fn code_chunks(display: &str, path: &Path, text: &str, budget: usize, max_symbol: usize, max_chunks: usize) -> Option<Vec<String>> {
    let lang = Lang::for_path(path).filter(|l| l.is_code())?;
    let symbols = top_level(&extract(lang, text));
    if symbols.is_empty() {
        return None;
    }

    let mut pieces: Vec<(Range<usize>, &str)> = Vec::new();
    let mut from = 0;
    for (i, s) in symbols.iter().enumerate() {
        let end = if i + 1 == symbols.len() { text.len() } else { s.bytes.end };
        pieces.push((from..end, s.name.as_str()));
        from = end;
    }

    let mut chunks = Vec::new();
    let mut current: Option<(Range<usize>, Vec<&str>)> = None;
    let flush = |current: &mut Option<(Range<usize>, Vec<&str>)>, chunks: &mut Vec<String>| {
        if let Some((range, names)) = current.take() {
            let body = text[range].trim_matches('\n');
            if !body.trim().is_empty() {
                chunks.push(format!("{display} · {}\n{body}", names.join(", ")));
            }
        }
    };

    for (range, name) in pieces {
        if range.len() > max_symbol {
            flush(&mut current, &mut chunks);
            let parts = split_lines(&text[range], max_symbol);
            let n = parts.len();
            for (i, part) in parts.into_iter().enumerate() {
                chunks.push(format!("{display} · {name} (part {} of {n})\n{}", i + 1, part.trim_matches('\n')));
            }
            continue;
        }
        match &mut current {
            Some((r, names)) if r.len() + range.len() <= budget => {
                r.end = range.end;
                names.push(name);
            }
            _ => {
                flush(&mut current, &mut chunks);
                current = Some((range, vec![name]));
            }
        }
    }
    flush(&mut current, &mut chunks);
    chunks.truncate(max_chunks);
    Some(chunks)
}

fn split_lines(text: &str, max: usize) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut last_break = 0;
    for (i, _) in text.match_indices('\n') {
        if i + 1 - start > max && last_break > start {
            parts.push(&text[start..last_break]);
            start = last_break;
        }
        last_break = i + 1;
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

// ---- COD-16: find_symbol ----

type Cache = Mutex<HashMap<PathBuf, (SystemTime, u64, Vec<Symbol>)>>;

fn cached_symbols(path: &Path, lang: Lang, text: &str) -> Vec<Symbol> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let stamp = std::fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
    if let Some((mtime, size)) = stamp {
        if let Some((m, s, symbols)) = cache.lock().unwrap().get(path) {
            if *m == mtime && *s == size {
                return symbols.clone();
            }
        }
    }
    let symbols = extract(lang, text);
    if let Some((mtime, size)) = stamp {
        let mut map = cache.lock().unwrap();
        if map.len() >= MAX_CACHED_FILES {
            map.clear();
        }
        map.insert(path.to_path_buf(), (mtime, size, symbols.clone()));
    }
    symbols
}

/// A whole-identifier occurrence: no identifier character on either side.
fn has_word(line: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    line.match_indices(word).any(|(i, _)| {
        !line[..i].chars().next_back().map_or(false, ident) && !line[i + word.len()..].chars().next().map_or(false, ident)
    })
}

fn clip(line: &str) -> String {
    let t = line.trim();
    if t.chars().count() > MAX_CONTEXT_CHARS {
        t.chars().take(MAX_CONTEXT_CHARS).collect::<String>() + "…"
    } else {
        t.to_string()
    }
}

/// `COD-16`: where `name` is defined, then where it is used, as `path:line`
/// with the line itself. Definitions come from the grammars. Uses are
/// whole-word text matches, which is also how a file in a language with no
/// grammar is searched: it still shows up, as a use.
pub fn find_symbol(root: &Path, name: &str, max: usize) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("missing 'name': the identifier to look up, e.g. `build_index`".into());
    }
    let files = if root.is_file() { vec![root.to_path_buf()] } else { super::index::walk_files(root) };
    let base = if root.is_file() { root.parent().unwrap_or(root) } else { root };

    let mut defs: Vec<String> = Vec::new();
    let mut loose: Vec<String> = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    let mut more_refs = false;
    let lower = name.to_lowercase();

    for path in &files {
        let Ok(meta) = std::fs::metadata(path) else { continue };
        if meta.len() > MAX_PARSE_BYTES || looks_binary(path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let rel = super::index::display_path(path, base);
        let lines: Vec<&str> = text.lines().collect();

        let mut def_lines = std::collections::HashSet::new();
        if let Some(lang) = Lang::for_path(path) {
            for s in cached_symbols(path, lang, &text) {
                let context = lines.get(s.line - 1).map(|l| clip(l)).unwrap_or_default();
                if s.name == name {
                    def_lines.insert(s.line);
                    defs.push(format!("{rel}:{}  {}  {context}", s.line, s.kind));
                } else if s.name.to_lowercase() == lower {
                    loose.push(format!("{rel}:{}  {} {}  {context}", s.line, s.kind, s.name));
                }
            }
        }
        for (i, line) in lines.iter().enumerate() {
            if def_lines.contains(&(i + 1)) || !has_word(line, name) {
                continue;
            }
            if refs.len() >= max {
                more_refs = true;
                break;
            }
            refs.push(format!("{rel}:{}  {}", i + 1, clip(line)));
        }
    }

    if defs.is_empty() && loose.is_empty() && refs.is_empty() {
        return Ok(format!(
            "No definition or use of `{name}` in {} file{}. Try search_files for a partial name.",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ));
    }
    let mut out = String::new();
    if defs.is_empty() {
        out.push_str(&format!("No definition of `{name}` found."));
    } else {
        out.push_str(&format!("Definitions of `{name}` ({}):\n{}", defs.len(), defs.join("\n")));
    }
    if !loose.is_empty() {
        out.push_str(&format!("\n\nSame name, different case:\n{}", loose.join("\n")));
    }
    if !refs.is_empty() {
        out.push_str(&format!("\n\nUses ({}{}):\n{}", refs.len(), if more_refs { "+" } else { "" }, refs.join("\n")));
    }
    if more_refs {
        out.push_str("\n… more uses; narrow `path` to a folder");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(lang: Lang, text: &str) -> Vec<(String, String)> {
        extract(lang, text).into_iter().map(|s| (s.name, s.kind)).collect()
    }

    fn has(list: &[(String, String)], name: &str, kind: &str) -> bool {
        list.iter().any(|(n, k)| n == name && k == kind)
    }

    #[test]
    fn every_tags_query_compiles_against_its_grammar() {
        for lang in [Lang::Rust, Lang::TypeScript, Lang::Tsx, Lang::JavaScript, Lang::Python, Lang::Go] {
            assert!(query(lang).is_some(), "{lang:?} tags query must compile");
        }
    }

    // COD-14-T: extraction per grammar.
    #[test]
    fn rust_symbols() {
        let s = names(
            Lang::Rust,
            "pub struct Card { a: u8 }\nenum Kind { A }\ntrait Run {}\nconst CAP: usize = 3;\nimpl Card {\n    fn new() -> Self { Card { a: 0 } }\n}\nfn detect() {}\nmod tests {}\n",
        );
        assert!(has(&s, "Card", "class"));
        assert!(has(&s, "Kind", "class"));
        assert!(has(&s, "Run", "interface"));
        assert!(has(&s, "CAP", "constant"));
        assert!(has(&s, "new", "method"), "{s:?}");
        assert!(!has(&s, "new", "function"), "one node is one symbol");
        assert!(has(&s, "detect", "function"));
        assert!(has(&s, "tests", "module"));
    }

    #[test]
    fn typescript_and_tsx_symbols() {
        let src = "export interface Props { a: number }\ntype Id = string;\nenum Mode { A }\nexport class Store { load() {} }\nexport function openItem() {}\nconst refresh = () => {};\n";
        for lang in [Lang::TypeScript, Lang::Tsx] {
            let s = names(lang, src);
            assert!(has(&s, "Props", "interface"), "{lang:?} {s:?}");
            assert!(has(&s, "Id", "type"));
            assert!(has(&s, "Mode", "enum"));
            assert!(has(&s, "Store", "class"));
            assert!(has(&s, "load", "method"));
            assert!(has(&s, "openItem", "function"));
            assert!(has(&s, "refresh", "function"));
        }
        let s = names(Lang::Tsx, "export function View() { return <div className=\"a\" />; }\n");
        assert!(has(&s, "View", "function"));
    }

    #[test]
    fn javascript_python_go_symbols() {
        let s = names(Lang::JavaScript, "class A { constructor() {} run() {} }\nfunction go() {}\n");
        assert!(has(&s, "A", "class"));
        assert!(has(&s, "run", "method"));
        assert!(!s.iter().any(|(n, _)| n == "constructor"));
        assert!(has(&s, "go", "function"));

        let s = names(Lang::Python, "LIMIT = 3\nclass Parser:\n    def parse(self):\n        pass\n\ndef main():\n    pass\n");
        assert!(has(&s, "LIMIT", "constant"));
        assert!(has(&s, "Parser", "class"));
        assert!(has(&s, "parse", "function"));
        assert!(has(&s, "main", "function"));

        let s = names(Lang::Go, "package main\ntype Server struct{}\nfunc (s *Server) Run() {}\nfunc main() {}\n");
        assert!(has(&s, "Server", "type"));
        assert!(has(&s, "Run", "method"));
        assert!(has(&s, "main", "function"));
    }

    #[test]
    fn json_keys_and_markdown_headings() {
        let s = names(Lang::Json, "{\"name\": \"app\", \"scripts\": {\"test\": \"vitest\"}}");
        assert_eq!(s, vec![("name".into(), "key".into()), ("scripts".into(), "key".into())]);

        let s = names(Lang::Markdown, "# Coding Plan\n\ntext\n\n## Phase 4 - Navigation\n\nmore\n");
        assert!(has(&s, "Coding Plan", "heading"), "{s:?}");
        assert!(has(&s, "Phase 4 - Navigation", "heading"));
    }

    #[test]
    fn top_level_drops_what_is_nested() {
        let all = extract(Lang::Python, "class A:\n    def m(self):\n        pass\n\ndef f():\n    pass\n");
        let top: Vec<String> = top_level(&all).into_iter().map(|s| s.name).collect();
        assert_eq!(top, vec!["A".to_string(), "f".to_string()]);
    }

    // COD-15-T: a chunk never splits a function.
    #[test]
    fn a_chunk_never_splits_a_function() {
        let mut src = String::from("use std::fmt;\n\n");
        for i in 0..12 {
            src.push_str(&format!("/// Doc for f{i}.\nfn f{i}(x: u32) -> u32 {{\n"));
            for j in 0..6 {
                src.push_str(&format!("    let v{j} = x + {j}; // padding padding padding\n"));
            }
            src.push_str("    x\n}\n\n");
        }
        let chunks = code_chunks("src/lib.rs", Path::new("lib.rs"), &src, 800, 4000, 60).unwrap();
        assert!(chunks.len() > 1, "the budget forces several chunks");
        for i in 0..12 {
            let head = format!("fn f{i}(");
            let holding: Vec<&String> = chunks.iter().filter(|c| c.contains(&head)).collect();
            assert_eq!(holding.len(), 1, "f{i} starts in exactly one chunk");
            let body = holding[0];
            let start = body.find(&head).unwrap();
            assert!(body[start..].contains("    x\n}"), "f{i} ends in the chunk it starts in");
            assert!(body.contains(&format!("/// Doc for f{i}.")), "its doc comment travels with it");
        }
        assert!(chunks[0].starts_with("src/lib.rs · f0"), "{}", chunks[0]);
        assert!(chunks[0].contains("use std::fmt;"), "the file's head is not lost");
    }

    #[test]
    fn only_an_oversized_function_is_cut_and_it_says_so() {
        let mut src = String::from("fn big() {\n");
        for i in 0..200 {
            src.push_str(&format!("    let a{i} = {i};\n"));
        }
        src.push_str("}\n");
        let chunks = code_chunks("big.rs", Path::new("big.rs"), &src, 800, 1000, 60).unwrap();
        assert!(chunks.len() > 1);
        assert!(chunks[0].starts_with(&format!("big.rs · big (part 1 of {})", chunks.len())));
    }

    #[test]
    fn prose_and_unknown_files_keep_the_old_chunker() {
        assert!(code_chunks("a.md", Path::new("a.md"), "# Title\n\ntext", 800, 4000, 60).is_none());
        assert!(code_chunks("a.cs", Path::new("a.cs"), "class A {}", 800, 4000, 60).is_none());
        assert!(code_chunks("a.rs", Path::new("a.rs"), "// only a comment\n", 800, 4000, 60).is_none());
    }

    #[test]
    fn whole_words_only() {
        assert!(has_word("let x = build_index(a);", "build_index"));
        assert!(!has_word("let x = rebuild_index(a);", "build_index"));
        assert!(!has_word("build_index_now()", "build_index"));
    }

    // COD-16-T: definitions rank above references.
    #[test]
    fn definitions_rank_above_uses() {
        let dir = std::env::temp_dir().join(format!("poiesis_sym_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src").join("a_use.rs"), "fn main() {\n    detect();\n}\n").unwrap();
        std::fs::write(dir.join("src").join("b_def.rs"), "pub fn detect() {}\n").unwrap();
        std::fs::write(dir.join("notes.cs"), "// calls detect from C#\n").unwrap();

        let out = find_symbol(&dir, "detect", 40).unwrap();
        let def = out.find("src/b_def.rs:1  function").expect(&out);
        let usage = out.find("src/a_use.rs:2").expect(&out);
        assert!(def < usage, "{out}");
        assert!(out.contains("notes.cs:1"), "a file with no grammar is still searched as text: {out}");
        assert!(!out.contains("src/b_def.rs:1  pub fn"), "the definition line is not repeated as a use: {out}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nothing_found_says_so() {
        let dir = std::env::temp_dir().join(format!("poiesis_sym_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
        let out = find_symbol(&dir, "missing_thing", 40).unwrap();
        assert!(out.starts_with("No definition or use of `missing_thing` in 1 file"), "{out}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
