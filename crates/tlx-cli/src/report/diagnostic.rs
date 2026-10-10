//! Evaluation errors and resource-limit diagnostics for a reported run.

use super::*;

/// The message of a full fingerprint set: what was found, what the current
/// `-fpmem` holds, and a suggestion (twice the memory given, or twice what
/// the states found so far would need); or, for a set that could not grow
/// within the memory budget, a larger `-maxmem`. Suggestions stay within
/// the machine's physical memory (`fp_suggestion`).
pub(crate) fn fp_set_full(mem: FpMem, distinct: u64, entries: u64) -> String {
    let phys = (tlx_runtime::mem::physical_bytes() >> 20) as u64;
    match mem {
        FpMem::Fixed(mib) => {
            // The run's own density, `entries` states per `mib` MiB (the set
            // is full at its load limit).
            let per_mib = (entries / (mib.max(1) as u64)).max(1);
            let needed = distinct.div_ceil(per_mib).max(1);
            let suggest = (mib as u64 * 2).max(needed * 2);
            format!(
                "{}\n  Distinct states found so far: {distinct}.\n  -fpmem {mib} MiB holds {entries} states.\n  {}",
                tlx_runtime::explore::FULL,
                fp_suggestion("-fpmem", mib as u64, suggest, phys)
            )
        }
        FpMem::Budget(mib) => format!(
            "{}\n  Distinct states found so far: {distinct}.\n  Within -maxmem {mib} MiB the fingerprint set holds {entries} states.\n  {}",
            tlx_runtime::explore::FULL_BUDGET,
            fp_suggestion("-maxmem", mib as u64, mib.max(1) as u64 * 2, phys)
        ),
    }
}

/// The suggestion of a full fingerprint set: option `opt` (now `cur` MiB)
/// at `want` MiB, or as much as the machine allows (90% of its `phys` MiB)
/// if that is less; none if `cur` is there already.
pub(crate) fn fp_suggestion(opt: &str, cur: u64, want: u64, phys: u64) -> String {
    let most = phys / 10 * 9;
    if want <= most {
        format!("Suggested {opt}: {want} MiB or more.")
    } else if cur < most {
        format!("Suggested {opt}: up to {most} MiB (this machine has {phys} MiB of memory).")
    } else {
        format!("This machine's memory ({phys} MiB) is not enough for this search in memory.")
    }
}

/// How TLC reports evaluation error `e` (raised while computing or checking
/// the initial states, in `run`): the message code, its text and the exit
/// status.
pub(crate) fn error_report(e: &ModelError, in_init: bool, run: &Run) -> (u32, String, u8) {
    let simulation = run.sim.is_some();
    let kind = out::error_kind(e);
    let msg = &e.message;
    match &e.context {
        ErrorContext::IncompleteState { action: None, state, .. } if simulation => (
            code::TLC_STATE_NOT_COMPLETELY_SPECIFIED_INITIAL,
            format!("State is not completely specified by the initial predicate:\n{state}\n"),
            EXIT_GENERAL_ERROR,
        ),
        // (A model checker finds it is not a "legal state".)
        ErrorContext::IncompleteState { action: None, state, .. } => (
            code::TLC_INITIAL_STATE,
            format!("current state is not a legal state\nWhile working on the initial state:\n{state}\n"),
            EXIT_GENERAL_ERROR,
        ),
        ErrorContext::IncompleteState { action: Some(a), missing, .. } => {
            let vars = format!("{} not defined: {}", if missing.len() > 1 { "s are" } else { " is" }, missing.join(", "));
            // (The action is named if there are several.)
            let text = match run.actions.get(*a as usize) {
                Some(name) if run.actions.len() > 1 => {
                    format!("Successor state is not completely specified by action {name} of the next-state relation. The following variable{vars}.\n")
                }
                _ => format!("Successor state is not completely specified by the next-state action. The following variable{vars}.\n"),
            };
            (code::TLC_STATE_NOT_COMPLETELY_SPECIFIED_NEXT, text, EXIT_EVAL_ERROR)
        }
        // (A model checker rethrows errors of a specific kind.)
        ErrorContext::InitialState(state) if simulation || kind == ErrorKind::General => {
            let exit = if simulation { EXIT_GENERAL_ERROR } else { EXIT_EVAL_ERROR };
            (code::TLC_INITIAL_STATE, format!("{}\nWhile working on the initial state:\n{state}\n", out::nested(kind, msg)), exit)
        }
        ErrorContext::Invariant(name) if !in_init => (
            code::TLC_INVARIANT_EVALUATION_FAILED,
            format!("Evaluating invariant {name} failed.\n{}", out::nested(kind, msg)),
            EXIT_SAFETY_EVAL,
        ),
        ErrorContext::ActionProperty(name) => {
            let exit = if simulation { EXIT_LIVENESS } else { EXIT_EVAL_ERROR };
            (
                code::TLC_ACTION_PROPERTY_EVALUATION_FAILED,
                format!("Evaluating action property {name} failed.\n{}", out::nested(kind, msg)),
                exit,
            )
        }
        // (TLC: Worker.doNextCheckLiveness fails with EC.GENERAL; an
        // `Assert` keeps its exit status.)
        ErrorContext::Liveness => {
            let exit = if kind == ErrorKind::AssertFailed { EXIT_ASSERT } else { EXIT_GENERAL_ERROR };
            (out::kind_code(kind), out::nested(kind, msg), exit)
        }
        _ if simulation => (code::GENERAL, out::unexpected_exception(kind, msg), EXIT_GENERAL_ERROR),
        _ if in_init => (code::GENERAL, out::nested(kind, msg), EXIT_EVAL_ERROR),
        ErrorContext::NextState if matches!(kind, ErrorKind::General | ErrorKind::Unsupported) => {
            (code::GENERAL, out::unexpected_exception(kind, msg), EXIT_EVAL_ERROR)
        }
        _ => {
            let exit = match kind {
                ErrorKind::General | ErrorKind::Unsupported => EXIT_EVAL_ERROR,
                ErrorKind::AssertFailed => EXIT_ASSERT,
                _ => EXIT_GENERAL_ERROR,
            };
            (out::kind_code(kind), msg.clone(), exit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fp_suggestion;

    #[test]
    fn fp_suggestions_stay_within_the_machine() {
        // 64 GiB of memory: at most 58,977 MiB suggested.
        let phys = 65536;
        assert_eq!(fp_suggestion("-maxmem", 512, 1024, phys), "Suggested -maxmem: 1024 MiB or more.");
        assert_eq!(
            fp_suggestion("-maxmem", 32768, 65536, phys),
            "Suggested -maxmem: up to 58977 MiB (this machine has 65536 MiB of memory)."
        );
        assert_eq!(
            fp_suggestion("-fpmem", 60000, 120000, phys),
            "This machine's memory (65536 MiB) is not enough for this search in memory."
        );
    }
}
