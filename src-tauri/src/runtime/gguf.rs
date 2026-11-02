//! `TIER-1`: a small reader for the key-value header of a `.gguf` file.
//!
//! Only what the window choice needs: the architecture, the size the model was
//! trained for, and the shape of its KV cache. And whether its chat template has
//! a place for tools, which places the model on a tier. Nothing in the app read
//! GGUF metadata before this.
//!
//! The layout is the one in the GGUF spec (`ggml-org/ggml`, `docs/gguf.md`), and
//! the key names are the ones llama.cpp `b11418` reads (`LLM_KV_NAMES`):
//! little-endian; magic `GGUF`; `u32` version; `u64` tensor count; `u64` pair
//! count; then pairs of a string key, a `u32` value type and the value. Keys have
//! no fixed order, so every pair is read and the `{arch}.*` ones are resolved
//! once `general.architecture` is known.
//!
//! Any problem gives `None`. Loading a model must never fail because of this
//! reader, so the caller falls back to the window it used before.

use std::collections::HashMap;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

const MAGIC: [u8; 4] = *b"GGUF";

// Value types, as the spec numbers them.
const T_STRING: u32 = 8;
const T_ARRAY: u32 = 9;

/// Keys and the architecture name are short. A longer one is a corrupt file, and
/// refusing it keeps a bad length from becoming a big allocation.
const MAX_TEXT: u64 = 1024;
/// Arrays nest in the spec. Nothing real nests past one level.
const MAX_DEPTH: usize = 4;
/// No real model has this many layers. Bounds the per-layer vectors.
const MAX_LAYERS: u32 = 4096;
/// Chat templates run to a few kilobytes. A bigger one is stepped over and counts
/// as unknown.
const MAX_TEMPLATE: u64 = 1 << 20;
const CHAT_TEMPLATE: &str = "tokenizer.chat_template";

/// What the window choice needs to know about a model file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufMeta {
    pub arch: String,
    /// `{arch}.context_length`: the window the model was trained for.
    pub trained_ctx: u32,
    pub n_layer: u32,
    pub n_embd: u32,
    /// One value per layer, whether the file stored a scalar or an array.
    pub head_count: Vec<u32>,
    /// One value per layer. A layer with 0 has no KV cache (hybrid models).
    pub head_count_kv: Vec<u32>,
    pub key_length: u32,
    pub value_length: u32,
    /// Whether the chat template has a place for tools. llama-server renders the
    /// tools through this template (`--jinja`); a template with none, like Gemma
    /// 3's, leaves the model to imitate calls as plain text. `None` when the file
    /// carries no template.
    pub template_tools: Option<bool>,
}

/// A wanted value, before the architecture is known.
enum Found {
    Int(u64),
    Ints(Vec<u64>),
}

/// The wanted keys by suffix after `{arch}.`.
const SUFFIXES: [&str; 7] = [
    ".context_length",
    ".embedding_length",
    ".block_count",
    ".attention.head_count",
    ".attention.head_count_kv",
    ".attention.key_length",
    ".attention.value_length",
];

type Cache = Mutex<HashMap<PathBuf, (SystemTime, Option<GgufMeta>)>>;

/// Read the header of `path`. Cached by `(path, mtime)`, so listing the library
/// does not read every file again each time.
pub fn read_header(path: &Path) -> Option<GgufMeta> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    if let Some((at, meta)) = cache.lock().ok()?.get(path) {
        if *at == mtime {
            return meta.clone();
        }
    }
    let meta = std::fs::File::open(path).ok().and_then(|f| {
        let len = f.metadata().ok()?.len();
        parse(BufReader::new(f), len)
    });
    if let Ok(mut cache) = cache.lock() {
        cache.insert(path.to_path_buf(), (mtime, meta.clone()));
    }
    meta
}

/// Whether a local model can be handed tools (`TIER-3`). Unknown, a file we
/// cannot read or one with no template, counts as yes, as it always did.
pub fn takes_tools(meta: Option<&GgufMeta>) -> bool {
    meta.and_then(|m| m.template_tools).unwrap_or(true)
}

/// A reader that knows where it is and how big the file is, so every length can
/// be checked before it is used.
struct Reader<R> {
    r: BufReader<R>,
    pos: u64,
    len: u64,
}

impl<R: Read + Seek> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        if self.pos.checked_add(N as u64)? > self.len {
            return None;
        }
        let mut buf = [0u8; N];
        self.r.read_exact(&mut buf).ok()?;
        self.pos += N as u64;
        Some(buf)
    }

    fn u32(&mut self) -> Option<u32> {
        self.bytes().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.bytes().map(u64::from_le_bytes)
    }

    /// Move forward `n` bytes without reading them.
    fn skip(&mut self, n: u64) -> Option<()> {
        let end = self.pos.checked_add(n)?;
        if end > self.len {
            return None;
        }
        // `seek_relative` keeps the buffer, which matters for the tokenizer
        // arrays: a hundred thousand tiny skips.
        self.r.seek_relative(i64::try_from(n).ok()?).ok()?;
        self.pos = end;
        Some(())
    }

    /// A string whose bytes are wanted. Short ones only.
    fn text(&mut self) -> Option<String> {
        let n = self.u64()?;
        if n > MAX_TEXT {
            return None;
        }
        String::from_utf8(self.bytes_of(n)?).ok()
    }

    /// The `n` bytes of a string whose length has been read.
    fn bytes_of(&mut self, n: u64) -> Option<Vec<u8>> {
        if self.pos.checked_add(n)? > self.len {
            return None;
        }
        let mut buf = vec![0u8; n as usize];
        self.r.read_exact(&mut buf).ok()?;
        self.pos += n;
        Some(buf)
    }

    /// An integer of any width, widened. `None` for a type that is not one.
    fn int(&mut self, ty: u32) -> Option<u64> {
        Some(match ty {
            0 => u64::from(self.bytes::<1>()?[0]),
            1 => i8::from_le_bytes(self.bytes()?).try_into().ok()?,
            2 => u64::from(u16::from_le_bytes(self.bytes()?)),
            3 => i16::from_le_bytes(self.bytes()?).try_into().ok()?,
            4 => u64::from(self.u32()?),
            5 => i32::from_le_bytes(self.bytes()?).try_into().ok()?,
            10 => self.u64()?,
            11 => i64::from_le_bytes(self.bytes()?).try_into().ok()?,
            _ => return None,
        })
    }

    /// Step over `count` elements of type `elem`, the body of an array whose
    /// header has been read.
    fn skip_elements(&mut self, elem: u32, count: u64, depth: usize) -> Option<()> {
        if depth >= MAX_DEPTH {
            return None;
        }
        if let Some(size) = fixed_size(elem) {
            return self.skip(count.checked_mul(size)?);
        }
        // A string or a nested array takes at least 8 bytes per element, so a
        // count that cannot fit in the rest of the file is corrupt.
        if count.checked_mul(8)? > self.len - self.pos {
            return None;
        }
        for _ in 0..count {
            self.skip_value(elem, depth + 1)?;
        }
        Some(())
    }

    /// Step over a value of type `ty` without keeping it.
    fn skip_value(&mut self, ty: u32, depth: usize) -> Option<()> {
        if let Some(size) = fixed_size(ty) {
            return self.skip(size);
        }
        match ty {
            T_STRING => {
                let n = self.u64()?;
                self.skip(n)
            }
            T_ARRAY => {
                let elem = self.u32()?;
                let count = self.u64()?;
                self.skip_elements(elem, count, depth)
            }
            _ => None,
        }
    }
}

/// Is `ty` one of the integer types (0 to 5, 10 and 11)?
fn is_int(ty: u32) -> bool {
    matches!(ty, 0..=5 | 10 | 11)
}

/// Bytes taken by a fixed-size value type, if it is one.
fn fixed_size(ty: u32) -> Option<u64> {
    match ty {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

/// Parse a header out of any reader. `len` is the size of the whole file.
fn parse<R: Read + Seek>(r: BufReader<R>, len: u64) -> Option<GgufMeta> {
    let mut c = Reader { r, pos: 0, len };
    if c.bytes::<4>()? != MAGIC {
        return None;
    }
    // Version 3 only added big-endian files over version 2, so the layout is the
    // same. A byte-swapped file reads its version as 0x03000000 and is refused.
    if !matches!(c.u32()?, 2 | 3) {
        return None;
    }
    let _tensors = c.u64()?;
    let pairs = c.u64()?;
    // A pair is a key length, a key and a type: at least 12 bytes.
    if pairs.checked_mul(12)? > len {
        return None;
    }

    let mut arch: Option<String> = None;
    let mut found: HashMap<String, Found> = HashMap::new();
    let mut template: Option<String> = None;
    for _ in 0..pairs {
        let key = c.text()?;
        let ty = c.u32()?;
        if key == "general.architecture" {
            if ty != T_STRING {
                return None;
            }
            arch = Some(c.text()?);
        } else if key == CHAT_TEMPLATE && ty == T_STRING {
            let n = c.u64()?;
            if n <= MAX_TEMPLATE {
                // Lossy: a template is read for one word, never shown.
                template = Some(String::from_utf8_lossy(&c.bytes_of(n)?).into_owned());
            } else {
                c.skip(n)?;
            }
        } else if SUFFIXES.iter().any(|s| key.ends_with(s)) {
            if ty == T_ARRAY {
                let elem = c.u32()?;
                let count = c.u64()?;
                // Only integer arrays are wanted, and only as long as a model has
                // layers. Anything else is stepped over.
                let size = is_int(elem).then(|| fixed_size(elem)).flatten();
                match size {
                    Some(size) if count <= u64::from(MAX_LAYERS) && count.checked_mul(size)? <= len - c.pos => {
                        let mut v = Vec::with_capacity(count as usize);
                        for _ in 0..count {
                            v.push(c.int(elem)?);
                        }
                        found.insert(key, Found::Ints(v));
                    }
                    _ => c.skip_elements(elem, count, 0)?,
                }
            } else if is_int(ty) {
                found.insert(key, Found::Int(c.int(ty)?));
            } else {
                c.skip_value(ty, 0)?;
            }
        } else {
            c.skip_value(ty, 0)?;
        }
    }

    let arch = arch?;
    let int = |suffix: &str| match found.get(&format!("{arch}{suffix}")) {
        Some(Found::Int(v)) => u32::try_from(*v).ok(),
        _ => None,
    };
    let trained_ctx = int(".context_length").filter(|n| *n > 0)?;
    let n_embd = int(".embedding_length").filter(|n| *n > 0)?;
    let n_layer = int(".block_count").filter(|n| (1..=MAX_LAYERS).contains(n))?;

    // A scalar is repeated for every layer; an array must have one entry per layer.
    let per_layer = |suffix: &str| -> Option<Option<Vec<u32>>> {
        match found.get(&format!("{arch}{suffix}")) {
            None => Some(None),
            Some(Found::Int(v)) => Some(Some(vec![u32::try_from(*v).ok()?; n_layer as usize])),
            Some(Found::Ints(v)) if v.len() == n_layer as usize => {
                v.iter().map(|n| u32::try_from(*n).ok()).collect::<Option<Vec<_>>>().map(Some)
            }
            Some(Found::Ints(_)) => None,
        }
    };
    let head_count = per_layer(".attention.head_count")??;
    let head_count_kv = per_layer(".attention.head_count_kv")?.unwrap_or_else(|| head_count.clone());

    // llama.cpp: `n_embd_head_k = n_embd / n_head`, for the first layer. A hybrid
    // model can have 0 there, so the first layer that has heads stands in.
    let heads = head_count.iter().copied().find(|n| *n > 0)?;
    let key_length = int(".attention.key_length").unwrap_or(n_embd / heads);
    let value_length = int(".attention.value_length").unwrap_or(n_embd / heads);

    // Qwen, Llama 3.1+ and Mistral templates take `tools` and render
    // `tool_calls`; a template that never mentions a tool cannot pass one on.
    let template_tools = template.map(|t| t.contains("tool"));

    Some(GgufMeta {
        arch,
        trained_ctx,
        n_layer,
        n_embd,
        head_count,
        head_count_kv,
        key_length,
        value_length,
        template_tools,
    })
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A GGUF header written by hand, so no model file is needed.

    pub enum V {
        U32(u32),
        U64(u64),
        Str(&'static str),
        Ints32(Vec<u32>),
        Strs(usize),
        Float(f32),
    }

    fn string(out: &mut Vec<u8>, s: &str) {
        out.extend((s.len() as u64).to_le_bytes());
        out.extend(s.as_bytes());
    }

    pub fn file(version: u32, pairs: &[(&str, V)]) -> Vec<u8> {
        let mut out = b"GGUF".to_vec();
        out.extend(version.to_le_bytes());
        out.extend(0u64.to_le_bytes());
        out.extend((pairs.len() as u64).to_le_bytes());
        for (key, value) in pairs {
            string(&mut out, key);
            match value {
                V::U32(n) => {
                    out.extend(4u32.to_le_bytes());
                    out.extend(n.to_le_bytes());
                }
                V::U64(n) => {
                    out.extend(10u32.to_le_bytes());
                    out.extend(n.to_le_bytes());
                }
                V::Str(s) => {
                    out.extend(8u32.to_le_bytes());
                    string(&mut out, s);
                }
                V::Ints32(v) => {
                    out.extend(9u32.to_le_bytes());
                    out.extend(4u32.to_le_bytes());
                    out.extend((v.len() as u64).to_le_bytes());
                    for n in v {
                        out.extend(n.to_le_bytes());
                    }
                }
                V::Strs(n) => {
                    out.extend(9u32.to_le_bytes());
                    out.extend(8u32.to_le_bytes());
                    out.extend((*n as u64).to_le_bytes());
                    for i in 0..*n {
                        string(&mut out, &format!("token{i}"));
                    }
                }
                V::Float(f) => {
                    out.extend(6u32.to_le_bytes());
                    out.extend(f.to_le_bytes());
                }
            }
        }
        out
    }

    /// A typical small model: 4 layers, 8 heads, 2 KV heads, 512 wide.
    pub fn llama() -> Vec<(&'static str, V)> {
        vec![
            ("general.architecture", V::Str("llama")),
            ("llama.context_length", V::U32(8192)),
            ("llama.embedding_length", V::U32(512)),
            ("llama.block_count", V::U32(4)),
            ("llama.attention.head_count", V::U32(8)),
            ("llama.attention.head_count_kv", V::U32(2)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    fn read(bytes: Vec<u8>) -> Option<GgufMeta> {
        let len = bytes.len() as u64;
        parse(BufReader::new(std::io::Cursor::new(bytes)), len)
    }

    #[test]
    fn reads_a_plain_header() {
        let m = read(file(3, &llama())).expect("a plain header reads");
        assert_eq!(m.arch, "llama");
        assert_eq!(m.trained_ctx, 8192);
        assert_eq!((m.n_layer, m.n_embd), (4, 512));
        assert_eq!(m.head_count, vec![8; 4]);
        assert_eq!(m.head_count_kv, vec![2; 4]);
        // Absent, so n_embd / n_head.
        assert_eq!((m.key_length, m.value_length), (64, 64));
    }

    /// A template that takes tools says so; one that never mentions a tool, like
    /// Gemma 3's, does not; a file with no template says nothing.
    #[test]
    fn reads_whether_the_chat_template_takes_tools() {
        let with = |t: &'static str| {
            let mut pairs = llama();
            pairs.insert(2, (CHAT_TEMPLATE, V::Str(t)));
            read(file(3, &pairs)).unwrap().template_tools
        };
        assert_eq!(with("{%- if tools %}{{ tools | tojson }}{%- endif %}"), Some(true));
        assert_eq!(with("{{ bos_token }}<start_of_turn>{{ message['content'] }}<end_of_turn>"), Some(false));
        assert_eq!(read(file(3, &llama())).unwrap().template_tools, None);
    }

    #[test]
    fn keys_in_any_order_with_the_architecture_last() {
        let mut pairs = llama();
        pairs.rotate_left(1); // the architecture moves to the end
        assert_eq!(pairs.last().unwrap().0, "general.architecture");
        assert_eq!(read(file(3, &pairs)).unwrap().trained_ctx, 8192);
    }

    #[test]
    fn a_large_string_array_before_the_wanted_keys_is_skipped() {
        let mut pairs = vec![("tokenizer.ggml.tokens", V::Strs(5000)), ("general.name", V::Str("x"))];
        pairs.extend(llama());
        pairs.insert(1, ("tokenizer.ggml.scores", V::Ints32(vec![0; 5000])));
        pairs.push(("tokenizer.ggml.f", V::Float(1.5)));
        assert_eq!(read(file(3, &pairs)).unwrap().n_layer, 4);
    }

    #[test]
    fn counts_may_be_u32_or_u64() {
        let mut pairs = llama();
        pairs[1] = ("llama.context_length", V::U64(32768));
        pairs[3] = ("llama.block_count", V::U64(4));
        let m = read(file(3, &pairs)).unwrap();
        assert_eq!((m.trained_ctx, m.n_layer), (32768, 4));
    }

    #[test]
    fn head_count_kv_as_a_scalar_an_array_and_absent() {
        let m = read(file(3, &llama())).unwrap();
        assert_eq!(m.head_count_kv, vec![2; 4]);

        let mut pairs = llama();
        pairs[5] = ("llama.attention.head_count_kv", V::Ints32(vec![2, 0, 2, 0]));
        assert_eq!(read(file(3, &pairs)).unwrap().head_count_kv, vec![2, 0, 2, 0]);

        let mut pairs = llama();
        pairs.pop();
        let m = read(file(3, &pairs)).unwrap();
        assert_eq!(m.head_count_kv, m.head_count, "defaults to head_count");
    }

    #[test]
    fn head_count_as_a_per_layer_array() {
        let mut pairs = llama();
        pairs[4] = ("llama.attention.head_count", V::Ints32(vec![0, 8, 8, 8]));
        pairs.pop();
        let m = read(file(3, &pairs)).unwrap();
        assert_eq!(m.head_count, vec![0, 8, 8, 8]);
        assert_eq!(m.key_length, 64, "the first layer with heads gives the default");
    }

    #[test]
    fn an_array_of_the_wrong_length_is_refused() {
        let mut pairs = llama();
        pairs[5] = ("llama.attention.head_count_kv", V::Ints32(vec![2, 2]));
        assert_eq!(read(file(3, &pairs)), None);
    }

    #[test]
    fn key_length_given_wins_over_the_default() {
        let mut pairs = llama();
        pairs.push(("llama.attention.key_length", V::U32(128)));
        pairs.push(("llama.attention.value_length", V::U32(96)));
        let m = read(file(3, &pairs)).unwrap();
        assert_eq!((m.key_length, m.value_length), (128, 96));
    }

    #[test]
    fn an_architecture_name_with_a_dash_or_an_underscore() {
        for arch in ["gpt-oss", "nemotron_h", "ernie4_5-moe"] {
            let key = |s: &str| -> &'static str { Box::leak(format!("{arch}{s}").into_boxed_str()) };
            let pairs = vec![
                ("general.architecture", V::Str(Box::leak(arch.to_string().into_boxed_str()))),
                (key(".context_length"), V::U32(4096)),
                (key(".embedding_length"), V::U32(256)),
                (key(".block_count"), V::U32(2)),
                (key(".attention.head_count"), V::U32(4)),
            ];
            assert_eq!(read(file(3, &pairs)).unwrap().arch, arch);
        }
    }

    #[test]
    fn another_architectures_keys_are_not_mistaken_for_this_ones() {
        let mut pairs = llama();
        pairs.push(("clip.context_length", V::U32(77)));
        assert_eq!(read(file(3, &pairs)).unwrap().trained_ctx, 8192);
    }

    #[test]
    fn version_2_is_accepted_and_the_rest_are_not() {
        assert!(read(file(2, &llama())).is_some());
        assert_eq!(read(file(1, &llama())), None);
        assert_eq!(read(file(4, &llama())), None);
        assert_eq!(read(file(3u32.swap_bytes(), &llama())), None, "a big-endian file");
    }

    #[test]
    fn not_a_gguf_file() {
        assert_eq!(read(b"RIFF....".to_vec()), None);
        assert_eq!(read(Vec::new()), None);
    }

    #[test]
    fn a_truncated_file_gives_nothing() {
        let bytes = file(3, &llama());
        for cut in [3, 10, 30, bytes.len() - 3] {
            assert_eq!(read(bytes[..cut].to_vec()), None, "cut at {cut}");
        }
    }

    #[test]
    fn an_oversized_length_gives_nothing_and_allocates_nothing() {
        // The first key claims to be 2^60 bytes long.
        let mut bytes = file(3, &llama());
        bytes[24..32].copy_from_slice(&(1u64 << 60).to_le_bytes());
        assert_eq!(read(bytes), None);

        // A pair count that cannot fit in the file.
        let mut bytes = file(3, &llama());
        bytes[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(read(bytes), None);

        // A string array that claims more elements than the file has bytes.
        let mut bytes = file(3, &[("tokenizer.ggml.tokens", V::Strs(2))]);
        let at = bytes.len() - (8 + 6) * 2 - 8;
        bytes[at..at + 8].copy_from_slice(&(1u64 << 50).to_le_bytes());
        assert_eq!(read(bytes), None);
    }

    #[test]
    fn a_missing_required_key_gives_nothing() {
        let mut pairs = llama();
        pairs.remove(1);
        assert_eq!(read(file(3, &pairs)), None);
    }

    #[test]
    fn reading_a_file_from_disk_is_cached_by_mtime() {
        let dir = std::env::temp_dir().join(format!("gguf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.gguf");
        std::fs::write(&path, file(3, &llama())).unwrap();
        assert_eq!(read_header(&path).unwrap().trained_ctx, 8192);
        assert!(read_header(&dir.join("missing.gguf")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
