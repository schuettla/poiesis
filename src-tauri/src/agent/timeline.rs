//! `HRW-5`: the saved step timeline, folded in Rust.
//!
//! The window used to build a turn's timeline itself, step by step as events
//! arrived, and save it with the message. This is that fold, ported one to one
//! from `streamAssistantTurn` (`src/lib/store.ts`) to the JSON shape of
//! `AgentStep` (`src/lib/types.ts`), so the turn saves its own timeline and the
//! window shows exactly what a reload would show.
//!
//! It reads events as JSON, the form the window gets them in, so a stream saved
//! as lines of JSON (`tests/fixtures/timeline`) folds through the same code a
//! live run does.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::agent::AgentEvent;

/// The id a turn's own recall step carries. A `Recall` event whose id matches no
/// step and starts with this is the "remembered" step, which goes first; any
/// other `Recall` hangs off the tool step it names, as before.
pub const RECALL_STEP_PREFIX: &str = "recall-";

/// What the timeline reads of an event. Anything else is not serialized for it.
pub fn reads(event: &AgentEvent) -> bool {
    matches!(
        event,
        AgentEvent::StepsParallel { .. }
            | AgentEvent::StepStart { .. }
            | AgentEvent::TaskStarted { .. }
            | AgentEvent::TaskOutput { .. }
            | AgentEvent::TaskEnded { .. }
            | AgentEvent::KeptResult { .. }
            | AgentEvent::StepDone { .. }
            | AgentEvent::StepError { .. }
            | AgentEvent::Recall { .. }
            | AgentEvent::Fired { .. }
            | AgentEvent::Code { .. }
            | AgentEvent::UntrustedFlag { .. }
            | AgentEvent::Untrusted { .. }
    )
}

/// An event as the window receives it: through its JSON text. Numbers come out
/// the way JavaScript reads them (an `f32` of 0.9 is 0.9, not 0.8999999761581421).
pub fn as_wire(event: &AgentEvent) -> Option<Value> {
    serde_json::from_str(&serde_json::to_string(event).ok()?).ok()
}

#[derive(Default)]
pub struct Timeline {
    steps: Vec<Value>,
    /// `HRN-UI-2`: step id -> the first id of the parallel batch it belongs to.
    /// Announced before the steps themselves arrive, so held until they do.
    parallel_of: HashMap<String, String>,
}

fn text<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

fn field(event: &Value, key: &str) -> Value {
    event.get(key).cloned().unwrap_or(Value::Null)
}

impl Timeline {
    pub fn steps(&self) -> &[Value] {
        &self.steps
    }

    fn step_mut(&mut self, id: &str) -> Option<&mut Map<String, Value>> {
        self.steps
            .iter_mut()
            .find(|s| s.get("id").and_then(Value::as_str) == Some(id))
            .and_then(Value::as_object_mut)
    }

    pub fn see(&mut self, event: &Value) {
        let Some(kind) = text(event, "type") else { return };
        let id = text(event, "id").unwrap_or_default().to_string();
        match kind {
            // `HRN-4`: named after its first step, which is stable and needs no counter.
            "steps_parallel" => {
                let ids: Vec<&str> = event
                    .get("ids")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                if let Some(first) = ids.first() {
                    for id in &ids {
                        self.parallel_of.insert((*id).to_string(), (*first).to_string());
                    }
                }
            }
            "step_start" => {
                let mut step = Map::new();
                step.insert("id".into(), json!(id));
                step.insert("verb".into(), field(event, "verb"));
                step.insert("target".into(), field(event, "target"));
                step.insert("status".into(), json!("running"));
                if let Some(group) = self.parallel_of.get(&id).filter(|g| !g.is_empty()) {
                    step.insert("parallelGroup".into(), json!(group));
                }
                if let Some(parent) = text(event, "parent").filter(|p| !p.is_empty()) {
                    step.insert("nestedUnder".into(), json!(parent));
                }
                self.steps.push(Value::Object(step));
            }
            // `COD-UI-2`: a task's progress hangs off its own step.
            "task_started" => {
                if let Some(step) = self.step_mut(&id) {
                    step.insert(
                        "task".into(),
                        json!({
                            "name": field(event, "task"),
                            "argv": field(event, "argv"),
                            "cwd": field(event, "cwd"),
                            "kind": field(event, "kind"),
                        }),
                    );
                }
            }
            "task_output" => {
                if let Some(task) = self.step_mut(&id).and_then(|s| s.get_mut("task")).and_then(Value::as_object_mut) {
                    task.insert("lastLine".into(), field(event, "line"));
                }
            }
            "task_ended" => {
                if let Some(step) = self.step_mut(&id) {
                    let target = step.get("target").cloned().unwrap_or(Value::Null);
                    let task = step
                        .entry("task")
                        .or_insert_with(|| json!({ "name": target, "argv": [], "cwd": "", "kind": "other" }));
                    if let Some(task) = task.as_object_mut() {
                        task.insert("outcome".into(), field(event, "outcome"));
                        task.insert("exitCode".into(), field(event, "exit_code"));
                        task.insert("durationMs".into(), field(event, "duration_ms"));
                        task.insert("diagnostics".into(), field(event, "diagnostics"));
                        task.insert("tail".into(), field(event, "tail"));
                    }
                }
            }
            // `HRN-8`: the model got a preview; the user gets all of it.
            "kept_result" => {
                if let Some(step) = self.step_mut(&id) {
                    step.insert(
                        "kept".into(),
                        json!({
                            "reference": field(event, "reference"),
                            "bytes": field(event, "bytes"),
                            "text": field(event, "text"),
                        }),
                    );
                }
            }
            "step_done" => {
                if let Some(step) = self.step_mut(&id) {
                    step.insert("status".into(), json!("done"));
                    match event.get("result").filter(|r| !r.is_null()) {
                        Some(result) => step.insert("result".into(), result.clone()),
                        None => step.remove("result"),
                    };
                }
            }
            "step_error" => {
                if let Some(step) = self.step_mut(&id) {
                    step.insert("status".into(), json!("error"));
                    step.insert("result".into(), json!(format!("— {}", text(event, "error").unwrap_or_default())));
                }
            }
            "recall" => {
                let matches = field(event, "matches");
                if let Some(step) = self.step_mut(&id) {
                    step.insert("matches".into(), matches);
                } else if id.starts_with(RECALL_STEP_PREFIX) {
                    if let Some(step) = remembered(&id, matches) {
                        self.steps.insert(0, step);
                    }
                }
            }
            // `HAB-UI-1`: the line goes in the step's detail.
            "fired" => {
                if let Some(step) = self.step_mut(&id) {
                    let fired = step.entry("fired").or_insert_with(|| json!([]));
                    if let Some(list) = fired.as_array_mut() {
                        list.push(json!({ "lesson": field(event, "lesson"), "line": field(event, "line") }));
                    }
                }
            }
            "code" => {
                if let Some(step) = self.step_mut(&id) {
                    step.insert("code".into(), json!({ "language": field(event, "language"), "code": field(event, "code") }));
                }
            }
            // `USE-8`: arrives after the text did, so it marks the source it is about.
            // A report with no `untrusted` event of its own gets a source with no text.
            "untrusted_flag" => {
                let label = text(event, "label").unwrap_or_default().to_string();
                let p = field(event, "p");
                if let Some(step) = self.step_mut(&id) {
                    let sources = step.entry("untrusted").or_insert_with(|| json!([]));
                    if let Some(list) = sources.as_array_mut() {
                        match list.iter_mut().find(|u| text(u, "label") == Some(label.as_str())) {
                            Some(source) => {
                                source["readsAsInstructions"] = p;
                            }
                            None => list.push(json!({
                                "label": label, "risk": 0, "flags": [], "text": "", "readsAsInstructions": p,
                            })),
                        }
                    }
                }
            }
            // `TRU-UI-1`: a step can wrap more than one source, so this accumulates.
            "untrusted" => {
                if let Some(step) = self.step_mut(&id) {
                    let sources = step.entry("untrusted").or_insert_with(|| json!([]));
                    if let Some(list) = sources.as_array_mut() {
                        list.push(json!({
                            "label": field(event, "label"),
                            "risk": field(event, "risk"),
                            "flags": field(event, "flags"),
                            "text": field(event, "text"),
                        }));
                    }
                }
            }
            _ => {}
        }
    }

    /// The `steps_json` of the turn: an array, or nothing when there are no steps.
    pub fn steps_json(&self) -> Option<String> {
        (!self.steps.is_empty()).then(|| Value::Array(self.steps.clone()).to_string())
    }
}

/// `SEM-5`/`HRW-3`: what surfaced on its own before the turn started, as a step
/// the timeline draws like a `search_history` call. `None` when nothing did.
fn remembered(id: &str, matches: Value) -> Option<Value> {
    let list = matches.as_array().filter(|m| !m.is_empty())?;
    let target = match list.as_slice() {
        [one] => text(one, "snippet").unwrap_or_default().to_string(),
        many => format!("{} things", many.len()),
    };
    Some(json!({ "id": id, "verb": "remembered", "target": target, "status": "done", "matches": matches }))
}

/// `VTN-6`: a spoken reply that was cut off keeps what was heard as its content,
/// and the whole reply goes beside the timeline. Said twice, the first whole
/// reply stays: the second call would only see what was heard.
pub fn with_unspoken(steps_json: Option<&str>, whole: &str) -> String {
    match steps_json.and_then(|s| serde_json::from_str::<Value>(s).ok()) {
        Some(Value::Object(o)) if o.get("unspoken").and_then(Value::as_str).is_some_and(|u| !u.is_empty()) => {
            Value::Object(o).to_string()
        }
        Some(Value::Array(steps)) => json!({ "steps": steps, "unspoken": whole }).to_string(),
        Some(Value::Object(mut o)) => {
            json!({ "steps": o.remove("steps").unwrap_or_else(|| json!([])), "unspoken": whole }).to_string()
        }
        _ => json!({ "steps": [], "unspoken": whole }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(events: &[Value]) -> Timeline {
        let mut t = Timeline::default();
        for e in events {
            t.see(e);
        }
        t
    }

    fn steps(t: &Timeline) -> Value {
        Value::Array(t.steps().to_vec())
    }

    #[test]
    fn a_step_runs_then_settles_with_its_result_or_its_error() {
        let t = fold(&[
            json!({"type":"step_start","id":"a","verb":"read","target":"src/x.rs"}),
            json!({"type":"step_start","id":"b","verb":"ran","target":"ls"}),
            json!({"type":"step_done","id":"a","result":"— 12 lines"}),
            json!({"type":"step_error","id":"b","error":"no such file"}),
            json!({"type":"step_start","id":"c","verb":"searched","target":"rust"}),
            json!({"type":"step_done","id":"c","result":null}),
        ]);
        assert_eq!(
            steps(&t),
            json!([
                {"id":"a","verb":"read","target":"src/x.rs","status":"done","result":"— 12 lines"},
                {"id":"b","verb":"ran","target":"ls","status":"error","result":"— no such file"},
                {"id":"c","verb":"searched","target":"rust","status":"done"},
            ])
        );
    }

    #[test]
    fn steps_announced_together_share_the_first_ones_group_and_scripts_nest() {
        let t = fold(&[
            json!({"type":"steps_parallel","ids":["a","b","c"]}),
            json!({"type":"step_start","id":"a","verb":"read","target":"1"}),
            json!({"type":"step_start","id":"b","verb":"read","target":"2"}),
            json!({"type":"step_start","id":"d","verb":"ran","target":"script"}),
            json!({"type":"step_start","id":"e","verb":"read","target":"3","parent":"d"}),
        ]);
        let s = steps(&t);
        assert_eq!(s[0]["parallelGroup"], "a");
        assert_eq!(s[1]["parallelGroup"], "a");
        assert!(s[2].get("parallelGroup").is_none());
        assert_eq!(s[3]["nestedUnder"], "d");
    }

    #[test]
    fn a_task_hangs_off_its_step_and_a_missing_start_gets_a_default() {
        let t = fold(&[
            json!({"type":"step_start","id":"a","verb":"ran","target":"cargo test"}),
            json!({"type":"task_output","id":"a","line":"ignored: no task yet"}),
            json!({"type":"task_started","id":"a","task":"cargo test","argv":["cargo","test"],"cwd":"/p","kind":"test","timeout_secs":60}),
            json!({"type":"task_output","id":"a","line":"running 3 tests"}),
            json!({"type":"task_ended","id":"a","outcome":"passed","exit_code":0,"timed_out":false,"cancelled":false,"duration_ms":900,"diagnostics":[],"tail":"ok"}),
            json!({"type":"step_start","id":"b","verb":"ran","target":"make"}),
            json!({"type":"task_ended","id":"b","outcome":"failed","exit_code":null,"duration_ms":5,"diagnostics":[],"tail":""}),
        ]);
        let s = steps(&t);
        assert_eq!(
            s[0]["task"],
            json!({"name":"cargo test","argv":["cargo","test"],"cwd":"/p","kind":"test","lastLine":"running 3 tests",
                   "outcome":"passed","exitCode":0,"durationMs":900,"diagnostics":[],"tail":"ok"})
        );
        assert_eq!(s[1]["task"]["name"], "make");
        assert_eq!(s[1]["task"]["kind"], "other");
        assert_eq!(s[1]["task"]["exitCode"], Value::Null);
    }

    #[test]
    fn what_hangs_off_a_step_is_kept_in_the_order_it_came() {
        let t = fold(&[
            json!({"type":"step_start","id":"a","verb":"ran","target":"code"}),
            json!({"type":"code","id":"a","language":"python","code":"print(1)"}),
            json!({"type":"kept_result","id":"a","reference":"res_1","bytes":90000,"text":"big"}),
            json!({"type":"fired","id":"a","lesson":"L1","line":"first"}),
            json!({"type":"fired","id":"a","lesson":"L2","line":"second"}),
            json!({"type":"untrusted","id":"a","label":"page at x.com","risk":2,"flags":["role"],"text":"hi"}),
            json!({"type":"untrusted_flag","id":"a","label":"page at x.com","p":0.9}),
            json!({"type":"untrusted_flag","id":"a","label":"email from y","p":0.5}),
            json!({"type":"fired","id":"nope","lesson":"L3","line":"lost"}),
        ]);
        let s = &steps(&t)[0];
        assert_eq!(s["code"], json!({"language":"python","code":"print(1)"}));
        assert_eq!(s["kept"], json!({"reference":"res_1","bytes":90000,"text":"big"}));
        assert_eq!(s["fired"], json!([{"lesson":"L1","line":"first"},{"lesson":"L2","line":"second"}]));
        assert_eq!(
            s["untrusted"],
            json!([
                {"label":"page at x.com","risk":2,"flags":["role"],"text":"hi","readsAsInstructions":0.9},
                {"label":"email from y","risk":0,"flags":[],"text":"","readsAsInstructions":0.5},
            ])
        );
    }

    fn hit(snippet: &str) -> Value {
        json!({"source":"memory","title":"t","created_at":1,"snippet":snippet})
    }

    #[test]
    fn the_remembered_step_goes_first_and_a_tool_recall_stays_on_its_step() {
        let t = fold(&[
            json!({"type":"step_start","id":"s1","verb":"searched","target":"x"}),
            json!({"type":"recall","id":"recall-1","matches":[hit("one thing")]}),
            json!({"type":"recall","id":"s1","matches":[hit("a"), hit("b")]}),
            json!({"type":"recall","id":"somewhere-else","matches":[hit("c")]}),
        ]);
        let s = steps(&t);
        assert_eq!(s.as_array().unwrap().len(), 2);
        assert_eq!(s[0], json!({"id":"recall-1","verb":"remembered","target":"one thing","status":"done","matches":[hit("one thing")]}));
        assert_eq!(s[1]["matches"].as_array().unwrap().len(), 2);

        let many = fold(&[json!({"type":"recall","id":"recall-2","matches":[hit("a"), hit("b"), hit("c")]})]);
        assert_eq!(steps(&many)[0]["target"], "3 things");
        assert!(fold(&[json!({"type":"recall","id":"recall-3","matches":[]})]).steps().is_empty());
    }

    #[test]
    fn the_saved_text_is_an_array_nothing_for_no_steps_and_keeps_what_was_not_heard() {
        assert_eq!(fold(&[]).steps_json(), None);
        let t = fold(&[json!({"type":"step_start","id":"a","verb":"read","target":"x"})]);
        let saved = t.steps_json().unwrap();
        assert!(saved.starts_with('['));

        let cut = serde_json::from_str::<Value>(&with_unspoken(Some(&saved), "the whole reply")).unwrap();
        assert_eq!(cut["unspoken"], "the whole reply");
        assert_eq!(cut["steps"][0]["id"], "a");
        let bare = serde_json::from_str::<Value>(&with_unspoken(None, "all of it")).unwrap();
        assert_eq!(bare, json!({"steps":[],"unspoken":"all of it"}));
        // Said twice, the first whole reply stays and the steps are kept.
        let again = serde_json::from_str::<Value>(&with_unspoken(Some(&cut.to_string()), "heard")).unwrap();
        assert_eq!(again["unspoken"], "the whole reply");
        assert_eq!(again["steps"][0]["id"], "a");
    }

    #[test]
    fn a_float_reads_the_way_the_window_reads_it() {
        let wire = as_wire(&AgentEvent::UntrustedFlag { id: "a".into(), label: "l".into(), p: 0.9 }).unwrap();
        assert_eq!(wire["p"], json!(0.9));
    }

    /// `HRW-T-3`: every fixture folds to the timeline saved for it. A fixture is a
    /// pair, `<name>.events.jsonl` (what the run sent) and `<name>.steps.json` (the
    /// `steps_json` of its row, or `null` for none). They are written by hand, one
    /// for each kind of thing a step can carry. A real turn that folds wrong is added
    /// as another pair.
    #[test]
    fn every_fixture_folds_to_the_timeline_the_window_saved() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/timeline");
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("fixtures/timeline")
            .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".events.jsonl").map(String::from))
            .collect();
        names.sort();
        assert!(names.len() >= 8, "expected the synthetic fixtures, found {names:?}");
        for name in &names {
            let events = std::fs::read_to_string(dir.join(format!("{name}.events.jsonl"))).unwrap();
            let mut t = Timeline::default();
            for line in events.lines().filter(|l| !l.trim().is_empty()) {
                t.see(&serde_json::from_str(line).unwrap_or_else(|e| panic!("{name}: {e}")));
            }
            let saved = std::fs::read_to_string(dir.join(format!("{name}.steps.json"))).unwrap();
            let want: Value = serde_json::from_str(&saved).unwrap();
            // The window saved an array, or `{steps, unspoken}`; only the steps are folded here.
            let want = match want {
                Value::Object(mut o) => o.remove("steps").unwrap_or(Value::Null),
                other => other,
            };
            let got = t.steps_json().map_or(Value::Null, |s| serde_json::from_str(&s).unwrap());
            assert_eq!(got, want, "fixture {name}");
        }
    }
}
