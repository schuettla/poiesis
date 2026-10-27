//! `USE-9`: the `classify` tool. The assistant hands over many items and one
//! closed question about each (which of a few answers, yes or no, a level on a
//! scale), and the small decision model answers every one in tens of
//! milliseconds on a card. That is where a fast judge beats generation: bulk
//! sorting, not the assistant's own next step.
//!
//! The assistant is told a summary and the rows the model was not sure about,
//! never all five hundred. The user gets the whole list as a `classify` block:
//! item, answer, confidence, sortable, with the low-confidence rows one click
//! away, and can correct a row, which is a label (`USE-UI-5`).
//!
//! Each item is one decision (`Purpose::Classify`), so each gets a row in the
//! log, a probability, and a label if the user corrects it. The controller counts
//! them against their own limit, not the twenty a turn (`control::CLASSIFY_PER_TURN`).
//!
//! The decision model sees an item's text and nothing else. A file's contents
//! never reach the assistant: only the path, the answer and the confidence do, so
//! this tool is not an intake of outside text.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::decide::control::{self, Act, Gate};
use super::decide::policy::Purpose;
use super::decide::sites::SiteAsk;
use super::decide::{self, Answer, Ask, DecideError, ImageRef, Opts, Question};
use super::toolsets::{self, ToolContext};

pub const TOOL: &str = "classify";
/// The most items one call takes. The plan's number; more is another call.
pub const MAX_ITEMS: usize = 500;
/// How much of an item the decision model reads.
const ITEM_CHARS: usize = 4_000;
/// Bytes read from a file for that: room for four-byte characters.
const READ_BYTES: u64 = 4 * ITEM_CHARS as u64;
/// How much of an item the table shows.
const LABEL_CHARS: usize = 80;
/// The low-confidence rows the assistant is told, beside the counts.
const LOW_LISTED: usize = 40;
/// With no table to look at (an unattended run) the assistant is told this many rows.
const ROWS_LISTED_UNSEEN: usize = 100;
/// How long one call may go on once the engine is up. A processor takes seconds an
/// item, so a long list is cut here and the rest named, not waited for.
const TIME_BUDGET: Duration = Duration::from_secs(240);
/// Failures in a row that say the engine is not working, not that one item is bad.
const MAX_FAILS_IN_A_ROW: usize = 5;
/// An image file larger than this is not read.
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
/// The longest side an image is sent at (the plan's 280 to 1,568 px range).
const MAX_IMAGE_SIDE: u32 = 1_568;
/// The longest side of a row's thumbnail. The app's asset protocol is limited to
/// its own generated images, so a user's picture cannot be shown by its path: a
/// small copy travels in the block instead.
const THUMB_SIDE: u32 = 48;
/// Rows that carry a thumbnail. About two kilobytes each, so a list of pictures
/// stays well inside the block's cap; the rest show their name only.
const THUMB_ROWS: usize = 100;
/// A table of five hundred rows is the design, not a tool dumping text.
const BLOCK_CAP_BYTES: usize = 512 * 1024;
/// The id of the one question each item is asked.
pub const ANSWER_ID: &str = "answer";
/// `decision_outcomes.source` of a row the user corrected or confirmed.
pub const USER_SOURCE: &str = "classify-user";

const NOT_READY: &str = "The quick decision model is not ready, so nothing was sorted. It needs Fast & slow thinking turned on in Settings \u{2192} Experiments and the decision model installed in Models. Do the sorting yourself, one item at a time.";
const NO_VISION: &str = "The decision model on this computer cannot read images (its vision file is missing), so nothing was sorted. Sort the images yourself, or sort a list of text.";

pub fn handles(name: &str) -> bool {
    name == TOOL
}

pub fn tool_specs() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": TOOL,
            "description": "Sort MANY items with ONE closed question about each, fast, on a small model on this computer. Use it for jobs like \"which of these 60 files touch auth?\", \"which tickets are bugs?\", \"which photos show a receipt?\". The answer to each item is one of a few options you give, yes or no, or a level on a scale. The user gets a table with an answer and a confidence for every item; you get a count per answer and the rows it was not sure about. Check those before you rely on them. Do NOT use it for one item, for your own next step, or for a question that needs reasoning over several items together. It reads at most the first 4000 characters of an item, so it is not for summarising.",
            "parameters": {
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The one question to answer about every item, in plain words, e.g. \"What kind of ticket is this?\"" },
                    "options": {
                        "type": "array",
                        "description": "The answers to pick one from, 2 to 255. Leave out for a yes or no question. Not together with `scale`.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "The answer, short, e.g. \"bug\"." },
                                "criteria": { "type": "string", "description": "When this answer is the right one." }
                            },
                            "required": ["id", "criteria"]
                        }
                    },
                    "scale": { "type": "array", "items": { "type": "string" }, "description": "Instead of options: 2 to 10 levels, lowest first, e.g. [\"low\", \"medium\", \"high\"]." },
                    "items": {
                        "type": "array",
                        "description": "What to sort, 1 to 500. Each item is a string of text, or {\"path\": \"a file to read as text\"}, or {\"image\": \"an image file\"}. An image needs the decision model's vision.",
                        "items": { "description": "text, {\"path\": ...} or {\"image\": ...}" }
                    },
                    "context": { "type": "string", "description": "OPTIONAL. Background that applies to every item, written once and kept short, e.g. what the project is." }
                },
                "required": ["question", "items"]
            }
        }
    }])
}

pub fn describe(_name: &str, args: &Value) -> (String, String) {
    match args.get("items").and_then(Value::as_array).map(Vec::len) {
        Some(1) => ("sorted".into(), "1 item".into()),
        Some(n) => ("sorted".into(), format!("{n} items")),
        None => ("sorted".into(), "a list".into()),
    }
}

// ---- the request ----

/// What kind of answer every item gets.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    YesNo,
    /// `(id, criteria)`, as the decision model reads them.
    Choice(Vec<(String, String)>),
    /// Levels, lowest first.
    Scale(Vec<String>),
}

impl Shape {
    fn name(&self) -> &'static str {
        match self {
            Shape::YesNo => "yes_no",
            Shape::Choice(_) => "choice",
            Shape::Scale(_) => "scale",
        }
    }

    /// The answers a person can correct a row to: the value a label carries and
    /// the words the table shows. The values are the ones calibration reads
    /// (`calibration::probs_of`): `true` and `false` for a yes or no, the option
    /// id, the level's index.
    pub fn choices(&self) -> Vec<(String, String)> {
        match self {
            Shape::YesNo => vec![("true".into(), "yes".into()), ("false".into(), "no".into())],
            Shape::Choice(options) => options.iter().map(|(id, _)| (id.clone(), id.clone())).collect(),
            Shape::Scale(levels) => levels.iter().enumerate().map(|(i, l)| (i.to_string(), l.clone())).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Text(String),
    File(String),
    Image(String),
}

impl Source {
    fn path(&self) -> Option<&str> {
        match self {
            Source::File(p) | Source::Image(p) => Some(p),
            Source::Text(_) => None,
        }
    }

    /// How the table names the item: its first words, or the tail of its path.
    fn label(&self) -> String {
        match self {
            Source::Text(t) => one_line(t, LABEL_CHARS),
            Source::File(p) | Source::Image(p) => tail(p, LABEL_CHARS),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub question: String,
    pub shape: Shape,
    pub items: Vec<Source>,
    pub context: String,
}

impl Job {
    fn has_images(&self) -> bool {
        self.items.iter().any(|i| matches!(i, Source::Image(_)))
    }

    pub fn question_of(&self) -> Question {
        match &self.shape {
            Shape::YesNo => yes_no(&self.question),
            Shape::Choice(options) => Question::Choice { instructions: self.question.clone(), options: options.clone() },
            Shape::Scale(levels) => Question::Score { instructions: self.question.clone(), levels: levels.clone() },
        }
    }
}

fn yes_no(question: &str) -> Question {
    Question::Noul {
        instructions: question.to_string(),
        when_true: "Yes, for this item.".into(),
        when_false: "No, not for this item.".into(),
    }
}

fn one_line(s: &str, n: usize) -> String {
    let squashed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if squashed.chars().count() <= n {
        return squashed;
    }
    let mut out: String = squashed.chars().take(n.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let kept: String = s.chars().skip(count - (n - 1)).collect();
    format!("\u{2026}{kept}")
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn text_arg<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn parse_options(raw: &Value) -> Result<Shape, String> {
    let list = raw.as_array().ok_or("`options` must be a list of answers, each with an `id` and `criteria`.")?;
    let mut options: Vec<(String, String)> = Vec::new();
    for (i, o) in list.iter().enumerate() {
        // A bare string is an answer whose name is its own criteria.
        let (id, criteria) = match o {
            Value::String(s) => (s.trim().to_string(), s.trim().to_string()),
            Value::Object(_) => {
                let id = text_arg(o, "id").or_else(|| text_arg(o, "name")).unwrap_or_default().to_string();
                let criteria = text_arg(o, "criteria").map(str::to_string).unwrap_or_else(|| id.clone());
                (id, criteria)
            }
            _ => (String::new(), String::new()),
        };
        if id.is_empty() {
            return Err(format!("Option {} has no `id`.", i + 1));
        }
        if options.iter().any(|(seen, _)| *seen == id) {
            return Err(format!("Two options are called \"{id}\". Each answer needs its own name."));
        }
        options.push((id, criteria));
    }
    if options.len() < 2 || options.len() > super::decide::local::MAX_OPTIONS {
        return Err(format!(
            "`options` needs 2 to {} answers; this has {}. For a yes or no question leave `options` out.",
            super::decide::local::MAX_OPTIONS,
            options.len()
        ));
    }
    Ok(Shape::Choice(options))
}

fn parse_scale(raw: &Value) -> Result<Shape, String> {
    let list = raw.as_array().ok_or("`scale` must be a list of levels, lowest first.")?;
    let levels: Vec<String> = list.iter().filter_map(|l| l.as_str()).map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    if levels.len() != list.len() || !(2..=10).contains(&levels.len()) {
        return Err("`scale` needs 2 to 10 levels, each a word or short phrase, lowest first.".into());
    }
    Ok(Shape::Scale(levels))
}

fn parse_item(i: usize, raw: &Value) -> Result<Source, String> {
    let n = i + 1;
    match raw {
        Value::String(s) if s.trim().is_empty() => Err(format!("Item {n} is empty.")),
        Value::String(s) => Ok(Source::Text(s.clone())),
        Value::Object(_) => {
            if let Some(p) = text_arg(raw, "path") {
                Ok(Source::File(p.to_string()))
            } else if let Some(p) = text_arg(raw, "image") {
                Ok(Source::Image(p.to_string()))
            } else if let Some(t) = text_arg(raw, "text") {
                Ok(Source::Text(t.to_string()))
            } else {
                Err(format!("Item {n} has none of `path`, `image` or `text`."))
            }
        }
        _ => Err(format!("Item {n} is not text, {{\"path\": ...}} or {{\"image\": ...}}.")),
    }
}

/// Read a call's arguments. Every refusal is in plain words, so the assistant can
/// fix the call and try again.
pub fn parse(args: &Value) -> Result<Job, String> {
    let question = text_arg(args, "question").ok_or("Give a `question`: the one thing to decide about every item.")?.to_string();
    let raw_items = args.get("items").and_then(Value::as_array).ok_or("Give `items`: the list of what to sort.")?;
    if raw_items.is_empty() {
        return Err("`items` is empty. There is nothing to sort.".into());
    }
    if raw_items.len() > MAX_ITEMS {
        return Err(format!(
            "At most {MAX_ITEMS} items in one call; this has {}. Sort the first {MAX_ITEMS}, then call again with the rest.",
            raw_items.len()
        ));
    }
    let options = args.get("options").filter(|v| !v.is_null());
    let scale = args.get("scale").filter(|v| !v.is_null());
    let shape = match (options, scale) {
        (Some(_), Some(_)) => return Err("Give `options` or `scale`, not both.".into()),
        (Some(o), None) => parse_options(o)?,
        (None, Some(s)) => parse_scale(s)?,
        (None, None) => Shape::YesNo,
    };
    let items = raw_items.iter().enumerate().map(|(i, it)| parse_item(i, it)).collect::<Result<Vec<_>, _>>()?;
    let context = text_arg(args, "context").map(|c| clip(c, ITEM_CHARS)).unwrap_or_default();
    Ok(Job { question, shape, items, context })
}

/// What the decision model reads for one item. The shared context comes first, so
/// every item of the call starts with the same text and the engine's prompt cache
/// is reused.
pub fn state_for(context: &str, body: &str) -> String {
    if context.trim().is_empty() {
        format!("Item:\n{body}")
    } else {
        format!("{context}\n\nItem:\n{body}")
    }
}

/// A labelled example in the form the calibration run asks it (`seed`): a pick-one
/// question, or with no options a yes or no one.
pub fn seed_ask(question: &str, options: Option<Vec<(String, String)>>, item: &str) -> SiteAsk {
    SiteAsk {
        state: state_for("", item),
        id: ANSWER_ID,
        question: match options {
            Some(options) => Question::Choice { instructions: question.to_string(), options },
            None => yes_no(question),
        },
    }
}

// ---- reading an answer ----

/// An answer as a row shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub value: String,
    pub label: String,
    /// The winner's own probability: the number the table shows, which a person
    /// can read as "about 8 in 10". Not the entropy measure of `decide::confidence`.
    pub confidence: f32,
}

pub fn read_answer(shape: &Shape, answer: &Answer) -> Option<Read> {
    match (shape, answer) {
        (Shape::YesNo, Answer::Noul { p_true }) => {
            let yes = *p_true >= 0.5;
            Some(Read {
                value: if yes { "true" } else { "false" }.into(),
                label: if yes { "yes" } else { "no" }.into(),
                confidence: p_true.max(1.0 - p_true),
            })
        }
        (Shape::Choice(options), Answer::Choice { choice, probs, .. }) => {
            options.iter().any(|(id, _)| id == choice).then(|| Read {
                value: choice.clone(),
                label: choice.clone(),
                confidence: probs.iter().find(|(id, _)| id == choice).map(|(_, p)| *p).unwrap_or(0.0),
            })
        }
        (Shape::Scale(levels), Answer::Score { probs, .. }) => {
            let (best, p) = probs.iter().copied().enumerate().fold((0, f32::MIN), |b, (i, p)| if p > b.1 { (i, p) } else { b });
            levels.get(best).map(|l| Read { value: best.to_string(), label: l.clone(), confidence: p.max(0.0) })
        }
        _ => None,
    }
}

// ---- rows, and what is told to whom ----

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Answered { read: Read, low: bool, decision: String },
    /// The item was not sorted, and why, in words for the user.
    Skipped(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub path: Option<String>,
    pub image: bool,
    /// A small JPEG as a data URL, for the first `THUMB_ROWS` image rows.
    pub thumb: Option<String>,
    pub outcome: Outcome,
}

impl Row {
    fn new(src: &Source, outcome: Outcome) -> Row {
        Row {
            label: src.label(),
            path: src.path().map(str::to_string),
            image: matches!(src, Source::Image(_)),
            thumb: None,
            outcome,
        }
    }

    fn answered(&self) -> bool {
        matches!(self.outcome, Outcome::Answered { .. })
    }

    fn low(&self) -> bool {
        matches!(self.outcome, Outcome::Answered { low: true, .. })
    }
}

fn two_places(c: f32) -> f64 {
    ((c * 100.0).round() as f64) / 100.0
}

/// How many rows each answer got, most first; ties in the order first seen.
pub fn counts(rows: &[Row]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for row in rows {
        if let Outcome::Answered { read, .. } = &row.outcome {
            match out.iter_mut().find(|(l, _)| *l == read.label) {
                Some((_, n)) => *n += 1,
                None => out.push((read.label.clone(), 1)),
            }
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// Everything the table block needs, in one value.
pub struct Report<'a> {
    pub job: &'a Job,
    pub rows: &'a [Row],
    pub bar: f32,
    pub calibrated: bool,
    /// Why the list was cut short, in words; `None` when every item was reached.
    pub stopped: Option<&'a str>,
}

impl Report<'_> {
    fn not_reached(&self) -> usize {
        self.job.items.len().saturating_sub(self.rows.len())
    }
}

/// The `classify` block (`USE-UI-5`): the rows with the answers a person can
/// correct a row to. The decision id is on each row so a correction can be a label.
pub fn block_data(r: &Report<'_>) -> Value {
    let rows: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            let mut v = json!({ "item": row.label });
            if let Some(p) = &row.path {
                v["path"] = json!(p);
            }
            if row.image {
                v["image"] = json!(true);
            }
            if let Some(t) = &row.thumb {
                v["thumb"] = json!(t);
            }
            match &row.outcome {
                Outcome::Answered { read, low, decision } => {
                    v["answer"] = json!(read.label);
                    v["value"] = json!(read.value);
                    v["confidence"] = json!(two_places(read.confidence));
                    v["low"] = json!(low);
                    v["decision"] = json!(decision);
                }
                Outcome::Skipped(why) => v["note"] = json!(why),
            }
            v
        })
        .collect();
    json!({
        "question": r.job.question,
        "kind": r.job.shape.name(),
        "choices": r.job.shape.choices().into_iter().map(|(value, label)| json!({ "value": value, "label": label })).collect::<Vec<_>>(),
        "bar": two_places(r.bar),
        "calibrated": r.calibrated,
        "total": r.job.items.len(),
        "not_reached": r.not_reached(),
        "stopped": r.stopped,
        "rows": rows,
    })
}

/// The line under the step: `60 sorted, 7 low confidence`.
pub fn step_note(rows: &[Row]) -> String {
    let sorted = rows.iter().filter(|r| r.answered()).count();
    let low = rows.iter().filter(|r| r.low()).count();
    let mut note = format!("\u{2014} {sorted} sorted");
    if low > 0 {
        note.push_str(&format!(", {low} low confidence"));
    }
    note
}

/// What the assistant is told: the counts, the rows the model was not sure about,
/// what was not sorted and why. With a table on screen it does not get the rest;
/// without one (an unattended run) it gets the rows, up to a limit.
pub fn summary_for_model(r: &Report<'_>, table_shown: bool) -> String {
    let answered = r.rows.iter().filter(|row| row.answered()).count();
    let mut out = format!(
        "Sorted {answered} of {} items. Question: \"{}\"\n",
        r.job.items.len(),
        r.job.question
    );
    let counted = counts(r.rows);
    if !counted.is_empty() {
        out.push_str(&format!(
            "Answers: {}.\n",
            counted.iter().map(|(l, n)| format!("{l} {n}")).collect::<Vec<_>>().join(", ")
        ));
    }
    let row_line = |row: &Row| -> Option<String> {
        match &row.outcome {
            Outcome::Answered { read, .. } => Some(format!("- {} \u{2192} {} ({:.2})", row.label, read.label, read.confidence)),
            Outcome::Skipped(_) => None,
        }
    };
    let low: Vec<&Row> = r.rows.iter().filter(|row| row.low()).collect();
    if !low.is_empty() {
        out.push_str(&format!("Not sure ({} rows with a confidence under {:.2}):\n", low.len(), r.bar));
        for row in low.iter().take(LOW_LISTED) {
            out.extend(row_line(row).map(|l| l + "\n"));
        }
        if low.len() > LOW_LISTED {
            out.push_str(&format!("- and {} more, in the table\n", low.len() - LOW_LISTED));
        }
    }
    let skipped: Vec<&Row> = r.rows.iter().filter(|row| matches!(row.outcome, Outcome::Skipped(_))).collect();
    if !skipped.is_empty() {
        out.push_str(&format!("{} items were not sorted:\n", skipped.len()));
        for row in skipped.iter().take(10) {
            if let Outcome::Skipped(why) = &row.outcome {
                out.push_str(&format!("- {}: {why}\n", row.label));
            }
        }
        if skipped.len() > 10 {
            out.push_str(&format!("- and {} more\n", skipped.len() - 10));
        }
    }
    if let Some(why) = r.stopped {
        out.push_str(&format!(
            "Stopped early: {why}. {} items were not reached; call classify again with the items from number {} on.\n",
            r.not_reached(),
            r.rows.len() + 1
        ));
    }
    if table_shown {
        out.push_str(
            "The user has the full table, with every answer and confidence, and can correct a row. Do not copy it into your answer. A small fast model gave these answers: check the rows marked not sure before you rely on them.\n",
        );
    } else {
        out.push_str("Every row (no table could be shown):\n");
        let confident: Vec<&Row> = r.rows.iter().filter(|row| row.answered() && !row.low()).collect();
        for row in confident.iter().take(ROWS_LISTED_UNSEEN) {
            out.extend(row_line(row).map(|l| l + "\n"));
        }
        if confident.len() > ROWS_LISTED_UNSEEN {
            out.push_str(&format!("- and {} more\n", confident.len() - ROWS_LISTED_UNSEEN));
        }
    }
    out
}

// ---- reading an item ----

/// What the decision model is given for one item.
enum Prepared {
    Text(String),
    /// The picture for the model, and a small copy for the table.
    Image { data_url: String, thumb: String },
}

impl Prepared {
    /// Two items with the same key get the same answer, so the second is not asked.
    fn key(&self, src: &Source) -> String {
        match self {
            Prepared::Text(t) => format!("t:{t}"),
            Prepared::Image { .. } => format!("i:{}", src.path().unwrap_or_default()),
        }
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    use std::io::Read as _;
    let meta = std::fs::metadata(path).map_err(|e| format!("couldn't open it ({e})"))?;
    if meta.is_dir() {
        return Err("it is a folder".into());
    }
    if super::filesystem::looks_binary(path) {
        return Err("it is not a text file".into());
    }
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(READ_BYTES).read_to_end(&mut buf))
        .map_err(|e| format!("couldn't read it ({e})"))?;
    Ok(clip(&String::from_utf8_lossy(&buf), ITEM_CHARS))
}

/// An image as a PNG data URL, turned upright by its EXIF orientation and scaled
/// down to the engine's range, and a thumbnail of it as a JPEG data URL for the
/// table. A file that is not an image is an error for that item only.
fn encode_image(path: &Path) -> Result<(String, String), String> {
    use base64::Engine as _;
    use image::ImageDecoder as _;
    let size = std::fs::metadata(path).map_err(|e| format!("couldn't open it ({e})"))?.len();
    if size > MAX_IMAGE_BYTES {
        return Err("it is larger than 16 MB".into());
    }
    let reader = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("couldn't open it ({e})"))?;
    let mut decoder = reader.into_decoder().map_err(|_| "it is not an image I can read".to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|_| "it is not an image I can read".to_string())?;
    img.apply_orientation(orientation);
    if img.width().max(img.height()) > MAX_IMAGE_SIDE {
        img = img.thumbnail(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE);
    }
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| format!("couldn't convert it ({e})"))?;
    // A JPEG has no transparency, so the thumbnail is flattened first.
    let small = image::DynamicImage::ImageRgb8(img.thumbnail(THUMB_SIDE, THUMB_SIDE).to_rgb8());
    let mut jpeg = Vec::new();
    small.write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).map_err(|e| format!("couldn't convert it ({e})"))?;
    let b64 = &base64::engine::general_purpose::STANDARD;
    Ok((format!("data:image/png;base64,{}", b64.encode(png)), format!("data:image/jpeg;base64,{}", b64.encode(jpeg))))
}

async fn prepare(
    ctx: &ToolContext<'_>,
    src: &Source,
    cleared: &mut HashMap<std::path::PathBuf, Result<(), String>>,
) -> Result<Prepared, String> {
    match src {
        Source::Text(t) => Ok(Prepared::Text(clip(t, ITEM_CHARS))),
        Source::File(raw) => {
            let path = super::filesystem::clear_read(ctx, raw, cleared).await?;
            Ok(Prepared::Text(read_text(&path)?))
        }
        Source::Image(raw) => {
            let path = super::filesystem::clear_read(ctx, raw, cleared).await?;
            let (data_url, thumb) = tauri::async_runtime::spawn_blocking(move || encode_image(&path))
                .await
                .map_err(|e| e.to_string())??;
            Ok(Prepared::Image { data_url, thumb })
        }
    }
}

// ---- the call ----

pub async fn execute(ctx: &ToolContext<'_>, name: &str, args: &Value) -> Result<String, String> {
    if name != TOOL {
        return Err(format!("unknown tool '{name}'"));
    }
    let job = parse(args)?;
    // This computer's engine, or the cloud when that is where quick decisions go.
    let local = crate::runtime::decideserver::resolve_local(ctx.mgr, ctx.db).await;
    let cloud = super::decide::cloud::chosen(ctx.db);
    if local.is_none() && !cloud {
        return Err(NOT_READY.into());
    }
    let cloud_sees = cloud && super::decide::cloud::sees_images(ctx.db, &super::decide::cloud::selected_model(ctx.db));
    if job.has_images() && !cloud_sees && local.as_ref().map_or(true, |l| l.mmproj.is_none()) {
        return Err(NO_VISION.into());
    }
    let policy = Purpose::Classify.policy();
    let opts = Opts { conversation_id: Some(ctx.conversation_id), run_id: None };
    let question = job.question_of();

    let mut cleared = HashMap::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut rows: Vec<Row> = Vec::new();
    let mut stopped: Option<String> = None;
    let mut calibrated = false;
    let mut bar = policy.act_at.at(false);
    let mut failed_in_a_row = 0usize;
    let mut files_read = 0usize;
    let mut thumbs_left = THUMB_ROWS;
    // Starts when the first item has been asked, so the engine's own start (up to
    // two minutes) is not taken out of the time the list gets.
    let mut clock: Option<Instant> = None;

    for src in &job.items {
        if ctx.cancel.is_some_and(|c| c.is_cancelled()) {
            stopped = Some("you stopped it".into());
            break;
        }
        if clock.is_some_and(|c| c.elapsed() >= TIME_BUDGET) {
            stopped = Some("the time for one list ran out".into());
            break;
        }
        let prepared = match prepare(ctx, src, &mut cleared).await {
            Ok(p) => p,
            Err(why) => {
                rows.push(Row::new(src, Outcome::Skipped(format!("couldn't read it: {why}"))));
                continue;
            }
        };
        if src.path().is_some() {
            files_read += 1;
        }
        // The same item twice is asked once and answered twice.
        let key = prepared.key(src);
        if let Some(&first) = seen.get(&key) {
            let mut copy = rows[first].clone();
            copy.label = src.label();
            copy.path = src.path().map(str::to_string);
            rows.push(copy);
            continue;
        }
        seen.insert(key, rows.len());

        let (state, images) = match &prepared {
            Prepared::Text(t) => (state_for(&job.context, t), Vec::new()),
            Prepared::Image { data_url, .. } => {
                (state_for(&job.context, "(the image attached)"), vec![ImageRef { data_url: data_url.clone() }])
            }
        };
        let ask = Ask { state: &state, images, questions: vec![(ANSWER_ID, question.clone())] };
        let got = decide::ask(&ask, Purpose::Classify, opts).await;
        clock.get_or_insert_with(Instant::now);
        match got {
            Ok(verdict) => {
                failed_in_a_row = 0;
                let read = verdict.get(ANSWER_ID).and_then(|a| read_answer(&job.shape, a).map(|r| (a, r)));
                let Some((answer, read)) = read else {
                    rows.push(Row::new(src, Outcome::Skipped("the model gave no usable answer".into())));
                    continue;
                };
                calibrated = verdict.calibrated;
                bar = policy.act_at.at(calibrated);
                let act = control::evaluate(Purpose::Classify, answer, calibrated, Gate::Open);
                decide::settle(&verdict, Purpose::Classify, act, opts);
                let mut row = Row::new(src, Outcome::Answered { read, low: act != Act::Go, decision: verdict.id.clone() });
                if let Prepared::Image { thumb, .. } = &prepared {
                    if thumbs_left > 0 {
                        row.thumb = Some(thumb.clone());
                        thumbs_left -= 1;
                    }
                }
                rows.push(row);
            }
            Err(DecideError::Budget(limit)) => {
                stopped = Some(limit.to_string());
                break;
            }
            Err(DecideError::NoVision) => return Err(NO_VISION.into()),
            Err(DecideError::Unavailable) => {
                if rows.iter().any(Row::answered) {
                    stopped = Some("the decision model stopped being available".into());
                    break;
                }
                return Err(NOT_READY.into());
            }
            Err(e) => {
                failed_in_a_row += 1;
                rows.push(Row::new(src, Outcome::Skipped(e.to_string())));
                if failed_in_a_row >= MAX_FAILS_IN_A_ROW {
                    stopped = Some(format!("the decision model failed {failed_in_a_row} times in a row ({e})"));
                    break;
                }
            }
        }
    }

    if files_read > 0 {
        let _ = ctx.db.log_activity(Some(ctx.conversation_id), "file", &format!("read {files_read} files to sort them"));
    }
    let report = Report { job: &job, rows: &rows, bar, calibrated, stopped: stopped.as_deref() };
    let title = format!("Sorted: {}", one_line(&job.question, 60));
    let block = toolsets::render_block_within(ctx, "classify", &title, &block_data(&report), BLOCK_CAP_BYTES);
    toolsets::set_step_note(ctx, step_note(&rows));
    Ok(summary_for_model(&report, block.is_some()))
}

// ---- a person's correction ----

/// `USE-UI-5`: a person picks an answer for a row. The same answer as the model's
/// is a `right` label, another is `corrected` with the answer it was changed to.
/// Either way it is written under the person's own source, so changing their mind
/// replaces it. Returns the label's word.
pub fn correct(db: &crate::db::Db, decision_id: &str, value: &str) -> Result<&'static str, String> {
    let row = db.decision_by_id(decision_id).map_err(|e| e.to_string())?.ok_or("That row's decision is no longer kept.")?;
    if row.purpose != Purpose::Classify.name() {
        return Err("That is not a sorted row.".into());
    }
    let (names, probs) = super::decide::calibration::probs_of(&row.answers).ok_or("That row has no answer to correct.")?;
    if !names.iter().any(|n| n == value) {
        return Err(format!("\"{value}\" is not one of the answers to that question."));
    }
    let winner = probs.iter().enumerate().fold(0, |b, (i, p)| if *p > probs[b] { i } else { b });
    let label = if names[winner] == value { "right" } else { "corrected" };
    db.set_outcome(decision_id, label, Some(value), USER_SOURCE).map_err(|e| e.to_string())?;
    Ok(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(args: Value) -> Result<Job, String> {
        parse(&args)
    }

    #[test]
    fn a_call_with_no_options_is_a_yes_or_no_question() {
        let j = job(json!({ "question": "Does it touch auth?", "items": ["a", { "path": "src/a.rs" }, { "image": "x.png" }] })).unwrap();
        assert_eq!(j.shape, Shape::YesNo);
        assert_eq!(j.items, vec![Source::Text("a".into()), Source::File("src/a.rs".into()), Source::Image("x.png".into())]);
        assert!(j.has_images());
        assert!(matches!(j.question_of(), Question::Noul { .. }));
    }

    #[test]
    fn options_become_a_pick_one_question_and_a_bare_string_names_its_own_criteria() {
        let j = job(json!({
            "question": "Kind?",
            "options": [{ "id": "bug", "criteria": "something is broken" }, "feature"],
            "items": ["x"],
            "context": "A tracker for a shop."
        }))
        .unwrap();
        assert_eq!(
            j.shape,
            Shape::Choice(vec![("bug".into(), "something is broken".into()), ("feature".into(), "feature".into())])
        );
        assert_eq!(j.context, "A tracker for a shop.");
        let Question::Choice { options, .. } = j.question_of() else { panic!() };
        assert_eq!(options.len(), 2);
    }

    #[test]
    fn a_scale_becomes_a_score_question_with_the_levels_as_answers() {
        let j = job(json!({ "question": "How urgent?", "scale": ["low", "medium", "high"], "items": ["x"] })).unwrap();
        assert_eq!(j.shape.choices(), vec![("0".into(), "low".into()), ("1".into(), "medium".into()), ("2".into(), "high".into())]);
        assert!(matches!(j.question_of(), Question::Score { .. }));
    }

    #[test]
    fn a_bad_call_is_refused_in_words_the_assistant_can_act_on() {
        let items = json!(["x"]);
        let err = |args: Value| job(args).unwrap_err();
        assert!(err(json!({ "items": items })).contains("`question`"));
        assert!(err(json!({ "question": "q" })).contains("`items`"));
        assert!(err(json!({ "question": "q", "items": [] })).contains("nothing to sort"));
        assert!(err(json!({ "question": "q", "items": items, "options": ["a", "b"], "scale": ["x", "y"] })).contains("not both"));
        assert!(err(json!({ "question": "q", "items": items, "options": ["only"] })).contains("2 to 255"));
        assert!(err(json!({ "question": "q", "items": items, "options": ["a", "a"] })).contains("\"a\""));
        assert!(err(json!({ "question": "q", "items": items, "scale": ["x"] })).contains("2 to 10"));
        assert!(err(json!({ "question": "q", "items": ["ok", "  "] })).contains("Item 2 is empty"));
        assert!(err(json!({ "question": "q", "items": ["ok", 7] })).contains("Item 2"));
        assert!(err(json!({ "question": "q", "items": [{ "name": "x" }] })).contains("Item 1"));
    }

    #[test]
    fn five_hundred_items_are_the_most_and_the_refusal_says_what_to_do() {
        let many = |n: usize| json!({ "question": "q", "items": vec!["x"; n] });
        assert!(job(many(MAX_ITEMS)).is_ok());
        let e = job(many(MAX_ITEMS + 1)).unwrap_err();
        assert!(e.contains("500") && e.contains("call again"), "{e}");
    }

    /// The shared context leads, so the engine's cache of the prefix is reused.
    #[test]
    fn every_items_state_starts_with_the_same_context() {
        let a = state_for("About the shop.", "first");
        let b = state_for("About the shop.", "second");
        let shared = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
        assert!(shared >= "About the shop.\n\nItem:\n".len());
        assert_eq!(state_for("", "x"), "Item:\nx");
        assert_eq!(state_for("  ", "x"), "Item:\nx");
    }

    #[test]
    fn an_answer_reads_as_the_winners_probability() {
        let yes_no = read_answer(&Shape::YesNo, &Answer::Noul { p_true: 0.2 }).unwrap();
        assert_eq!((yes_no.value.as_str(), yes_no.label.as_str()), ("false", "no"));
        assert!((yes_no.confidence - 0.8).abs() < 1e-6, "the likelier side, whichever it is");

        let shape = Shape::Choice(vec![("bug".into(), "x".into()), ("feature".into(), "y".into())]);
        let choice = Answer::Choice {
            choice: "feature".into(),
            probs: vec![("bug".into(), 0.3), ("feature".into(), 0.7)],
            confidence: 0.1,
        };
        let r = read_answer(&shape, &choice).unwrap();
        assert_eq!((r.value.as_str(), r.confidence), ("feature", 0.7));

        let scale = Shape::Scale(vec!["low".into(), "medium".into(), "high".into()]);
        let r = read_answer(&scale, &Answer::Score { score: 1.4, probs: vec![0.1, 0.3, 0.6], confidence: 0.2 }).unwrap();
        assert_eq!((r.value.as_str(), r.label.as_str(), r.confidence), ("2", "high", 0.6));
    }

    /// An answer that is not one of the options, or of another kind, is not a row.
    #[test]
    fn an_answer_that_does_not_fit_the_question_is_not_read() {
        let shape = Shape::Choice(vec![("bug".into(), "x".into()), ("feature".into(), "y".into())]);
        let off = Answer::Choice { choice: "other".into(), probs: vec![("other".into(), 1.0)], confidence: 1.0 };
        assert!(read_answer(&shape, &off).is_none());
        assert!(read_answer(&shape, &Answer::Noul { p_true: 0.9 }).is_none());
        assert!(read_answer(&Shape::Scale(vec!["a".into(), "b".into()]), &Answer::Noul { p_true: 0.9 }).is_none());
    }

    fn answered(item: &str, label: &str, confidence: f32, low: bool) -> Row {
        Row {
            label: item.into(),
            path: None,
            image: false,
            thumb: None,
            outcome: Outcome::Answered {
                read: Read { value: label.into(), label: label.into(), confidence },
                low,
                decision: format!("d-{item}"),
            },
        }
    }

    fn sample_job(n: usize) -> Job {
        Job {
            question: "Kind?".into(),
            shape: Shape::Choice(vec![("bug".into(), "broken".into()), ("feature".into(), "new".into())]),
            items: vec![Source::Text("x".into()); n],
            context: String::new(),
        }
    }

    #[test]
    fn the_assistant_is_told_the_counts_and_the_unsure_rows_not_every_row() {
        let j = sample_job(5);
        let rows = vec![
            answered("one", "bug", 0.95, false),
            answered("two", "bug", 0.9, false),
            answered("three", "feature", 0.55, true),
            answered("four", "bug", 0.52, true),
            Row { label: "five".into(), path: Some("f.bin".into()), image: false, thumb: None, outcome: Outcome::Skipped("it is not a text file".into()) },
        ];
        let report = Report { job: &j, rows: &rows, bar: 0.7, calibrated: false, stopped: None };
        let text = summary_for_model(&report, true);
        assert!(text.contains("Sorted 4 of 5 items"), "{text}");
        assert!(text.contains("Answers: bug 3, feature 1."), "{text}");
        assert!(text.contains("Not sure (2 rows with a confidence under 0.70)"), "{text}");
        assert!(text.contains("- three \u{2192} feature (0.55)") && text.contains("- four \u{2192} bug (0.52)"), "{text}");
        assert!(!text.contains("one \u{2192}"), "a confident row is not listed when the table is shown: {text}");
        assert!(text.contains("five: it is not a text file"), "{text}");
        assert!(text.contains("Do not copy it"), "{text}");
    }

    #[test]
    fn with_no_table_shown_the_assistant_gets_the_confident_rows_too() {
        let j = sample_job(2);
        let rows = vec![answered("one", "bug", 0.95, false), answered("two", "feature", 0.5, true)];
        let report = Report { job: &j, rows: &rows, bar: 0.7, calibrated: true, stopped: None };
        let text = summary_for_model(&report, false);
        assert!(text.contains("Every row") && text.contains("- one \u{2192} bug (0.95)"), "{text}");
    }

    #[test]
    fn a_list_cut_short_says_where_to_pick_it_up() {
        let j = sample_job(10);
        let rows: Vec<Row> = (0..4).map(|i| answered(&format!("r{i}"), "bug", 0.9, false)).collect();
        let report = Report { job: &j, rows: &rows, bar: 0.7, calibrated: false, stopped: Some("the time for one list ran out") };
        let text = summary_for_model(&report, true);
        assert!(text.contains("Stopped early: the time for one list ran out. 6 items were not reached"), "{text}");
        assert!(text.contains("from number 5 on"), "{text}");
        assert_eq!(report.not_reached(), 6);
    }

    #[test]
    fn a_long_unsure_list_is_cut_and_the_rest_counted() {
        let j = sample_job(60);
        let rows: Vec<Row> = (0..60).map(|i| answered(&format!("r{i}"), "bug", 0.5, true)).collect();
        let report = Report { job: &j, rows: &rows, bar: 0.7, calibrated: false, stopped: None };
        let text = summary_for_model(&report, true);
        assert_eq!(text.matches("\u{2192} bug (0.50)").count(), LOW_LISTED);
        assert!(text.contains("and 20 more, in the table"), "{text}");
    }

    #[test]
    fn the_block_carries_every_row_with_the_decision_a_correction_needs() {
        let j = sample_job(3);
        let rows = vec![
            answered("one", "bug", 0.8312, false),
            Row {
                label: "pic.png".into(),
                path: Some("C:/pics/pic.png".into()),
                image: true,
                thumb: Some("data:image/jpeg;base64,AAAA".into()),
                outcome: Outcome::Answered {
                    read: Read { value: "feature".into(), label: "feature".into(), confidence: 0.4 },
                    low: true,
                    decision: "d-pic".into(),
                },
            },
            Row { label: "bad".into(), path: Some("bad.txt".into()), image: false, thumb: None, outcome: Outcome::Skipped("it is a folder".into()) },
        ];
        let report = Report { job: &j, rows: &rows, bar: 0.7, calibrated: false, stopped: None };
        let data = block_data(&report);
        assert_eq!(data["kind"], "choice");
        assert_eq!(data["bar"], 0.7);
        assert_eq!(data["total"], 3);
        assert_eq!(data["not_reached"], 0);
        assert_eq!(data["choices"][1], json!({ "value": "feature", "label": "feature" }));
        assert_eq!(data["rows"][0]["confidence"], 0.83, "two places, not an f32 tail");
        assert_eq!(data["rows"][0]["decision"], "d-one");
        assert_eq!(data["rows"][1]["image"], true);
        assert_eq!(data["rows"][1]["thumb"], "data:image/jpeg;base64,AAAA");
        assert!(data["rows"][0].get("thumb").is_none(), "only a row that has one carries it");
        assert_eq!(data["rows"][1]["path"], "C:/pics/pic.png");
        assert_eq!(data["rows"][1]["low"], true);
        assert_eq!(data["rows"][2]["note"], "it is a folder");
        assert!(data["rows"][2].get("answer").is_none());
    }

    /// Five hundred rows is the design: the block must fit the cap it is given,
    /// with a thumbnail on every row that gets one.
    #[test]
    fn five_hundred_rows_fit_the_blocks_cap() {
        let j = sample_job(MAX_ITEMS);
        let thumb = format!("data:image/jpeg;base64,{}", "A".repeat(2_800));
        let rows: Vec<Row> = (0..MAX_ITEMS)
            .map(|i| Row {
                label: format!("{:<80}", format!("tickets/long-folder-name/ticket-{i}.md")),
                path: Some(format!("C:/Users/someone/workspace/project/tickets/long-folder-name/ticket-{i}.md")),
                image: true,
                thumb: (i < THUMB_ROWS).then(|| thumb.clone()),
                outcome: Outcome::Answered {
                    read: Read { value: "feature".into(), label: "feature".into(), confidence: 0.4 },
                    low: true,
                    decision: uuid::Uuid::new_v4().to_string(),
                },
            })
            .collect();
        let report = Report { job: &j, rows: &rows, bar: 0.7, calibrated: false, stopped: None };
        let size = serde_json::to_string(&block_data(&report)).unwrap().len();
        assert!(size < BLOCK_CAP_BYTES, "{size} bytes");
    }

    #[test]
    fn the_step_line_counts_what_was_sorted_and_what_was_unsure() {
        let rows = vec![answered("a", "bug", 0.9, false), answered("b", "bug", 0.5, true)];
        assert_eq!(step_note(&rows), "\u{2014} 2 sorted, 1 low confidence");
        assert_eq!(step_note(&rows[..1]), "\u{2014} 1 sorted");
    }

    #[test]
    fn counts_come_most_first() {
        let rows = vec![answered("a", "feature", 0.9, false), answered("b", "bug", 0.9, false), answered("c", "bug", 0.9, false)];
        assert_eq!(counts(&rows), vec![("bug".to_string(), 2), ("feature".to_string(), 1)]);
    }

    #[test]
    fn labels_are_short_one_liners_and_paths_keep_their_tail() {
        assert_eq!(Source::Text("a  b\n\nc".into()).label(), "a b c");
        let long = Source::Text("word ".repeat(40)).label();
        assert_eq!(long.chars().count(), LABEL_CHARS);
        assert!(long.ends_with('\u{2026}'));
        let path = Source::File(format!("{}/file.rs", "dir/".repeat(40))).label();
        assert_eq!(path.chars().count(), LABEL_CHARS);
        assert!(path.starts_with('\u{2026}') && path.ends_with("file.rs"));
    }

    #[test]
    fn the_tool_is_advertised_once_and_handled() {
        let specs = tool_specs();
        let spec = specs.as_array().unwrap();
        assert_eq!(spec.len(), 1);
        assert_eq!(spec[0].pointer("/function/name").and_then(Value::as_str), Some(TOOL));
        assert_eq!(spec[0].pointer("/function/parameters/required"), Some(&json!(["question", "items"])));
        assert!(handles("classify") && !handles("read_file"));
        assert_eq!(describe(TOOL, &json!({ "items": [1, 2, 3] })), ("sorted".to_string(), "3 items".to_string()));
        assert_eq!(describe(TOOL, &json!({ "items": [1] })).1, "1 item");
    }

    #[test]
    fn a_text_file_is_read_up_to_a_limit_and_a_binary_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.txt");
        std::fs::write(&text, "x".repeat(ITEM_CHARS * 3)).unwrap();
        assert_eq!(read_text(&text).unwrap().chars().count(), ITEM_CHARS);
        let bin = dir.path().join("a.bin");
        std::fs::write(&bin, [0u8, 1, 2, 3]).unwrap();
        assert!(read_text(&bin).unwrap_err().contains("not a text file"));
        assert!(read_text(dir.path()).unwrap_err().contains("folder"));
        assert!(read_text(&dir.path().join("gone.txt")).is_err());
    }

    #[test]
    fn an_image_becomes_a_png_data_url_scaled_into_range_and_a_non_image_is_refused() {
        use base64::Engine as _;
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.png");
        image::ImageBuffer::from_fn(2_000, 1_000, |x, _| image::Rgb([(x % 255) as u8, 10, 10])).save(&big).unwrap();
        let (url, thumb) = encode_image(&big).unwrap();
        let b64 = url.strip_prefix("data:image/png;base64,").expect("a png data url");
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), MAX_IMAGE_SIDE, "the long side is brought into range");
        assert!(decoded.height() < decoded.width(), "and the shape is kept");

        // The table gets a small JPEG of its own, not the picture the model reads.
        let b64 = thumb.strip_prefix("data:image/jpeg;base64,").expect("a jpeg data url");
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert!(bytes.len() < 4_000, "{} bytes", bytes.len());
        let small = image::load_from_memory(&bytes).unwrap();
        assert_eq!(small.width(), THUMB_SIDE);
        assert!(small.height() < small.width());

        let text = dir.path().join("notes.png");
        std::fs::write(&text, "not an image at all").unwrap();
        assert!(encode_image(&text).unwrap_err().contains("not an image"));
    }

    #[test]
    fn a_seed_row_is_one_question_with_the_items_state() {
        let ask = seed_ask("Kind?", Some(vec![("bug".into(), "broken".into()), ("feature".into(), "new".into())]), "It crashes.");
        assert_eq!(ask.id, ANSWER_ID);
        assert_eq!(ask.state, "Item:\nIt crashes.");
        assert!(super::super::decide::local::check_ask(&ask.as_ask()).is_ok());
        let yes_no = seed_ask("Refund?", None, "I want my money back.");
        assert!(matches!(yes_no.question, Question::Noul { .. }));
    }

    // ---- a person's correction ----

    fn db_with_decision(purpose: &str, answers: Value) -> (crate::db::Db, String) {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.add_decision(&crate::db::decisions::NewDecision {
            id: "d1",
            conversation_id: None,
            run_id: None,
            purpose,
            backend: "local:Clef-flash",
            model: "Clef-Flash-Q4_K_M",
            ms: 40,
            calibrated: false,
            answers: &answers,
            input_tokens: 10,
            state_preview: "p",
            state_hash: "h",
            action: None,
        })
        .unwrap();
        (db, "d1".into())
    }

    fn bug_answer() -> Value {
        json!({ "answer": { "choice": "bug", "probs": [["bug", 0.7], ["feature", 0.3]], "confidence": 0.1 } })
    }

    #[test]
    fn choosing_the_models_answer_is_a_right_label_and_another_is_a_correction() {
        let (db, id) = db_with_decision("classify", bug_answer());
        assert_eq!(correct(&db, &id, "feature"), Ok("corrected"));
        let row = db.decision_by_id(&id).unwrap().unwrap();
        assert_eq!((row.outcomes[0].label.as_str(), row.outcomes[0].value.as_deref()), ("corrected", Some("feature")));
        // Changing their mind replaces the label: one row, not two.
        assert_eq!(correct(&db, &id, "bug"), Ok("right"));
        let row = db.decision_by_id(&id).unwrap().unwrap();
        assert_eq!(row.outcomes.len(), 1);
        assert_eq!(row.outcomes[0].label, "right");
        // And the label feeds calibration as it is: a correction names the true class.
        let labels = super::super::decide::outcomes::labels_for(&db, &row);
        assert_eq!(labels[0].source, USER_SOURCE);
        assert!(super::super::decide::outcomes::observation(&row, &labels).is_some());
    }

    #[test]
    fn a_yes_or_no_row_is_corrected_with_true_or_false() {
        let (db, id) = db_with_decision("classify", json!({ "answer": { "p": 0.9 } }));
        assert_eq!(correct(&db, &id, "false"), Ok("corrected"));
        assert_eq!(correct(&db, &id, "true"), Ok("right"));
    }

    #[test]
    fn only_a_sorted_row_can_be_corrected_and_only_to_one_of_its_answers() {
        let (db, id) = db_with_decision("goal-met", bug_answer());
        assert!(correct(&db, &id, "bug").unwrap_err().contains("not a sorted row"));
        let (db, id) = db_with_decision("classify", bug_answer());
        assert!(correct(&db, &id, "nonsense").unwrap_err().contains("not one of the answers"));
        assert!(correct(&db, "missing", "bug").unwrap_err().contains("no longer kept"));
        let (db, id) = db_with_decision("classify", json!({}));
        assert!(correct(&db, &id, "bug").unwrap_err().contains("no answer"));
    }
}
