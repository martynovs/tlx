//! Helpers shared by the paranoid (`TLX_PARANOID`) cross-checks of the
//! compiled model (`enc::model::EncModel`): they compare its per-state
//! results with the interpreter's and fail fast with both lists.

use tlx_eval::InterpModel;
use tlx_value::Value;

/// Report a paranoid mismatch (compiled / IR code and the interpreter
/// disagree) by panicking: the search stops at once and the process fails
/// (see `tlx_runtime::explore`, which propagates worker panics).
#[cold]
#[track_caller]
pub(crate) fn mismatch(what: std::fmt::Arguments) -> ! {
    panic!("TLX_PARANOID: compiled code differs from the interpreter: {what}")
}

/// One state per line: `x = .., y = ..`.
pub(crate) fn state_list(base: &InterpModel, states: &[Vec<Value>]) -> String {
    let vars = &base.ip.prog.vars;
    let line = |s: &Vec<Value>| {
        let parts: Vec<String> = s.iter().zip(vars.iter()).map(|(v, d)| format!("{} = {v}", d.name)).collect();
        format!("  {}", parts.join(", "))
    };
    if states.is_empty() {
        return "  (none)".into();
    }
    states.iter().map(line).collect::<Vec<_>>().join("\n")
}

/// Paranoid: fail unless the multisets of states `want` (interpreter) and
/// `got` (compiled / IR) are equal; `what` describes them.
pub(crate) fn compare_states(base: &InterpModel, what: &dyn Fn() -> String, mut want: Vec<Vec<Value>>, mut got: Vec<Vec<Value>>) {
    want.sort();
    got.sort();
    if want == got {
        return;
    }
    // The differences (multiset differences of the sorted lists).
    let (mut missing, mut extra) = (Vec::new(), Vec::new());
    let (mut a, mut b) = (want.iter().peekable(), got.iter().peekable());
    loop {
        match (a.peek(), b.peek()) {
            (None, None) => break,
            (Some(x), Some(y)) if x == y => {
                a.next();
                b.next();
            }
            (Some(x), Some(y)) if x < y => missing.push(a.next().unwrap().clone()),
            (Some(_), None) => missing.push(a.next().unwrap().clone()),
            _ => extra.push(b.next().unwrap().clone()),
        }
    }
    mismatch(format_args!(
        "{}\ninterpreter ({} states):\n{}\ncompiled ({} states):\n{}\nmissing in compiled ({}):\n{}\nextra in compiled ({}):\n{}",
        what(),
        want.len(),
        state_list(base, &want),
        got.len(),
        state_list(base, &got),
        missing.len(),
        state_list(base, &missing),
        extra.len(),
        state_list(base, &extra)
    ));
}
