//! `-dumpTrace json|tla FILE`: after a safety violation, write the
//! counterexample as TLC's `_JsonTrace` / `_TLAPlusCounterExample`
//! postconditions do.
//!
//! TLC's `CounterExample` is the record `[action |-> A, state |-> S]`: S the
//! set of `<<i, state>>` (state i of the behavior, a record of the
//! variables in the order TLC shows them), A the set of
//! `<<<<i, s>>, action, <<i + 1, t>>>>` with `action` the record
//! `[name |-> ..., location |-> [beginLine |-> ..., beginColumn |-> ...,
//! endLine |-> ..., endColumn |-> ..., module |-> ...]]` plus
//! `context |-> [param |-> value, ...], parameters |-> <<"param", ...>>`
//! for an action with parameters (tlc2.value.impl.CounterExample,
//! RecordValue(Action)). The fields stay in that (construction) order, the
//! sets are ordered by state number. The json format is
//! `JsonSerialize(FILE, [counterexample |-> CounterExample, vars |-> V])`
//! (V the set of variable names); the tla format is the module
//! `---- MODULE <FILE without its first ".tla"> ----` defining
//! `CounterExample == <ToString(CounterExample)>`. TLC writes both for
//! invariant, action property, deadlock and liveness violations, not after
//! evaluation errors. A liveness counterexample (a lasso) has one more
//! step, from its last state back to the state its cycle starts with
//! (`<<<<n, s>>, action, <<k, t>>>>`; a stuttering cycle is a step from the
//! last state to itself, with the action of the step into that state:
//! LiveWorker.printTrace, CounterExample(trace, action, loopOrdinal)). The
//! lasso is the one tlx shows (liveness traces are tier 3, see
//! docs/compat.md §0).

use std::sync::{Arc, Mutex, OnceLock};

use tlx_eval::model::{ActionInfo, InterpModel, Location};
use tlx_sema::source::Field;
use tlx_value::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Tla,
}

static REQUESTS: OnceLock<Vec<(Format, String)>> = OnceLock::new();

/// A liveness counterexample as shown: its states (values) with the action
/// of the step into each (None: an initial state), and the step closing
/// the lasso: to state number `back_to` (1-based), by action `back` (None:
/// an unknown action, TLC's `UnnamedAction`).
#[derive(Clone)]
pub struct Lasso {
    pub states: Vec<(Arc<[Value]>, Option<u32>)>,
    pub back_to: usize,
    pub back: Option<u32>,
}

/// The lassos shown while `-dumpTrace` is requested, by the text showing
/// them (a liveness violation is reported by its text).
static LASSOS: Mutex<Vec<(String, Lasso)>> = Mutex::new(Vec::new());

/// Keep `lasso`, shown as `text`, for `-dumpTrace` (if requested).
pub fn keep_lasso(text: &str, lasso: Lasso) {
    if requested() {
        LASSOS.lock().unwrap_or_else(|e| e.into_inner()).push((text.to_string(), lasso));
    }
}

/// The lasso shown as `text`, if kept.
pub fn kept_lasso(text: &str) -> Option<Lasso> {
    LASSOS.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().find(|(t, _)| t == text).map(|(_, l)| l.clone())
}

/// The `-dumpTrace` options of the command line, in their order.
pub fn set(requests: Vec<(Format, String)>) {
    let _ = REQUESTS.set(requests);
}

pub fn requested() -> bool {
    REQUESTS.get().is_some_and(|r| !r.is_empty())
}

/// A JSON value (`ToJson`).
fn json(v: &Value) -> String {
    match tlx_eval::natives::json::to_json(v) {
        Ok(Value::Str(s)) => s.as_str().to_string(),
        _ => "null".to_string(),
    }
}

fn json_string(s: &str) -> String {
    json(&Value::str(s))
}

/// The fields of a record, kept in the given order.
fn json_record(fields: &[(String, String)]) -> String {
    let fs: Vec<String> = fields.iter().map(|(k, v)| format!("{}:{v}", json_string(k))).collect();
    format!("{{{}}}", fs.join(","))
}

fn tla_record(fields: &[(String, String)]) -> String {
    let fs: Vec<String> = fields.iter().map(|(k, v)| format!("{k} |-> {v}")).collect();
    format!("[{}]", fs.join(", "))
}

/// How a format writes a value and a record (its fields, in order).
#[derive(Clone, Copy)]
struct Fmt<'a> {
    value: &'a dyn Fn(&Value) -> String,
    record: &'a dyn Fn(&[(String, String)]) -> String,
}

impl Fmt<'static> {
    fn of(format: Format) -> Self {
        match format {
            Format::Json => Fmt { value: &json, record: &json_record },
            Format::Tla => Fmt { value: &|v: &Value| v.to_string(), record: &tla_record },
        }
    }
}

/// The location record `l`.
fn location_fields(l: &Location, f: Fmt<'_>) -> String {
    let fields: Vec<(String, String)> = l
        .fields()
        .into_iter()
        .map(|(n, fld)| {
            (
                n.to_string(),
                (f.value)(&match fld {
                    Field::Int(i) => Value::Int(i),
                    Field::Str(m) => Value::str(m),
                }),
            )
        })
        .collect();
    (f.record)(&fields)
}

/// The action record's fields.
fn action_fields(a: &ActionInfo, f: Fmt<'_>) -> Vec<(String, String)> {
    let loc = location_fields(&a.location, f);
    let mut fields = vec![("name".to_string(), (f.value)(&Value::str(&a.name))), ("location".to_string(), loc)];
    if !a.params.is_empty() {
        let ctx: Vec<(String, String)> = a.params.iter().map(|(n, v)| (n.clone(), (f.value)(v))).collect();
        fields.push(("context".into(), (f.record)(&ctx)));
        fields.push(("parameters".into(), (f.value)(&Value::tuple(a.params.iter().map(|(n, _)| Value::str(n)).collect()))));
    }
    fields
}

/// TLC's unknown action (`Action.UNKNOWN`): the action of a stuttering
/// step closing a lasso at an initial state.
fn unnamed_fields(f: Fmt<'_>) -> Vec<(String, String)> {
    let loc = location_fields(&Location::builtins(), f);
    vec![("name".to_string(), (f.value)(&Value::str("UnnamedAction"))), ("location".to_string(), loc)]
}

/// The counterexample in one format: `(state values, action)` per state,
/// and for a lasso the closing step (to state number `k`, by an action).
fn render(
    base: &InterpModel,
    steps: &[(std::sync::Arc<[Value]>, Option<u32>)],
    close: Option<(usize, Option<u32>)>,
    format: Format,
) -> String {
    let fmt = Fmt::of(format);
    let (value, record) = (fmt.value, fmt.record);
    let pair = |i: usize, s: &[Value]| {
        let fields: Vec<(String, String)> = base.state_fields(s).into_iter().map(|(n, v)| (n.to_string(), value(v))).collect();
        match format {
            Format::Json => format!("[{},{}]", i + 1, record(&fields)),
            Format::Tla => format!("<<{}, {}>>", i + 1, record(&fields)),
        }
    };
    let states: Vec<String> = steps.iter().enumerate().map(|(i, (s, _))| pair(i, s)).collect();
    let edge = |from: &str, act: String, to: &str| match format {
        Format::Json => format!("[{from},{act},{to}]"),
        Format::Tla => format!("<<{from}, {act}, {to}>>"),
    };
    let mut actions: Vec<String> = (1..steps.len())
        .map(|i| {
            let a = steps[i].1.map(|a| base.action_info(a as usize));
            let act = a.map_or_else(|| record(&[]), |a| record(&action_fields(&a, fmt)));
            edge(&states[i - 1], act, &states[i])
        })
        .collect();
    if let (Some((k, a)), Some(last)) = (close, states.last()) {
        let fields = a.map_or_else(|| unnamed_fields(fmt), |a| action_fields(&base.action_info(a as usize), fmt));
        actions.push(edge(last, record(&fields), &states[k.clamp(1, states.len()) - 1]));
    }
    match format {
        Format::Json => {
            let mut names: Vec<Value> = base.ip.prog.vars.iter().map(|v| Value::str(&v.name)).collect();
            names.dedup();
            let vars = Value::set_from(names).map(|v| json(&v)).unwrap_or_else(|_| "[]".into());
            let ce =
                json_record(&[("action".into(), format!("[{}]", actions.join(","))), ("state".into(), format!("[{}]", states.join(",")))]);
            json_record(&[("counterexample".into(), ce), ("vars".into(), vars)])
        }
        Format::Tla => {
            tla_record(&[("action".into(), format!("{{{}}}", actions.join(", "))), ("state".into(), format!("{{{}}}", states.join(", ")))])
        }
    }
}

/// Write the requested files for a counterexample (`(state values,
/// action)` per state) and report each as TLC does (`PrintT`).
pub fn write(base: &InterpModel, steps: &[(std::sync::Arc<[Value]>, Option<u32>)]) {
    write_steps(base, steps, None);
}

/// `write` for a liveness counterexample.
pub fn write_lasso(base: &InterpModel, l: &Lasso) {
    write_steps(base, &l.states, Some((l.back_to, l.back)));
}

fn write_steps(base: &InterpModel, steps: &[(std::sync::Arc<[Value]>, Option<u32>)], close: Option<(usize, Option<u32>)>) {
    let Some(requests) = REQUESTS.get() else { return };
    for (format, file) in requests {
        let text = render(base, steps, close, *format);
        let result = match format {
            // JsonSerialize: creates the parent directories.
            Format::Json => tlx_runtime::io::write_creating_parents(std::path::Path::new(file), text.as_bytes()),
            // IOUtils!Serialize with format TXT; the module is named after
            // the file name without its first ".tla".
            Format::Tla => {
                let module = file.replacen(".tla", "", 1);
                std::fs::write(file, format!("---- MODULE {module} ----\nLOCAL INSTANCE TLC\n\nCounterExample == \n\t{text}\n\n===="))
            }
        };
        match result {
            Ok(()) => eprintln!("{}", Value::str(&format!("CounterExample written: {file}"))),
            Err(e) => eprintln!("Error: -dumpTrace could not write {file}: {e}"),
        }
    }
}
