//! Diagnostic output: named topics, enabled by `TLX_*` knobs, written as
//! `topic: text` lines to one sink.
//!
//! This is the channel of everything a developer reads to see what the
//! compiled tier did: progress of the `-compile` run (`compile:` lines,
//! always on), the typed-IR coverage report, why functions are not native,
//! IR dumps, JIT disassembly, and so on. It is not the channel of TLC's
//! messages (`tlx-cli`'s `out.rs`): those are part of the checker's output
//! and never go through here.
//!
//! - `Topic::Compile` (and `Coverage`, which is part of the same report)
//!   is the *progress stream*: a front end installs its sink with
//!   `install_progress` (the CLI writes to stdout, or to stderr with
//!   `-tool`). Without a sink these lines are dropped (library users and
//!   tests).
//! - The other topics are debugging switches: off unless their knob is
//!   set, and then written to stderr.
//!
//! `enabled` is a cached load: a call site that has to build text first
//! asks it, `emit` checks it itself.

use std::fmt::Display;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use crate::knobs::Env;

/// A diagnostic topic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    /// Progress of the compiled tier (`compile: ...`); always on.
    Compile,
    /// `TLX_COVERAGE`: variable layouts and the histogram of boxed /
    /// interpreted IR nodes; part of the `compile:` report.
    Coverage,
    /// `TLX_CGEN_DEBUG`: why functions are not native (`cgen: ...`).
    Cgen,
    /// `TLX_EQ_DEBUG`: types and shapes of boxed equalities.
    Eq,
    /// `TLX_IR_DUMP=NAME`: the IR of actions / predicates called NAME.
    Ir,
    /// `TLX_JIT_DISASM`: machine code of the JIT functions.
    Jit,
    /// `TLX_BAILOUT_DEBUG=N`: the first N errors of the IR executor.
    Bailout,
}

const TOPICS: [Topic; 7] = [Topic::Compile, Topic::Coverage, Topic::Cgen, Topic::Eq, Topic::Ir, Topic::Jit, Topic::Bailout];

impl Topic {
    /// The prefix of the topic's lines.
    pub fn prefix(self) -> &'static str {
        match self {
            Topic::Compile | Topic::Coverage => "compile",
            Topic::Cgen => "cgen",
            Topic::Eq => "eq",
            Topic::Ir => "ir",
            Topic::Jit => "jit",
            Topic::Bailout => "bailout",
        }
    }

    /// The knob that turns the topic on (`None`: always on).
    pub fn knob(self) -> Option<&'static str> {
        match self {
            Topic::Compile => None,
            Topic::Coverage => Some("TLX_COVERAGE"),
            Topic::Cgen => Some("TLX_CGEN_DEBUG"),
            Topic::Eq => Some("TLX_EQ_DEBUG"),
            Topic::Ir => Some("TLX_IR_DUMP"),
            Topic::Jit => Some("TLX_JIT_DISASM"),
            Topic::Bailout => Some("TLX_BAILOUT_DEBUG"),
        }
    }

    /// Whether the topic goes to the progress stream (else to stderr).
    fn progress(self) -> bool {
        matches!(self, Topic::Compile | Topic::Coverage)
    }

    fn bit(self) -> u64 {
        1 << self as u32
    }
}

/// The topics the knobs of the process turn on, as a bit set (computed on
/// first use).
fn knob_bits() -> u64 {
    static BITS: OnceLock<u64> = OnceLock::new();
    *BITS.get_or_init(|| {
        let env = Env::process();
        TOPICS.iter().filter(|t| t.knob().is_some_and(|k| from_env(&env, **t, k))).fold(0, |b, t| b | t.bit())
    })
}

fn from_env(env: &Env, t: Topic, knob: &str) -> bool {
    match t {
        Topic::Ir => env.string(knob).is_some_and(|s| !s.is_empty()),
        Topic::Bailout => env.num(knob).is_ok_and(|n| n > 0),
        _ => env.flag(knob),
    }
}

/// Topics forced on or off for tests: `(mask, value)`.
#[cfg(test)]
static OVERRIDE: (AtomicU64, AtomicU64) = (AtomicU64::new(0), AtomicU64::new(0));

/// Force `topic` on or off, whatever its knob says (tests only).
#[cfg(test)]
fn set_for_test(topic: Topic, on: bool) {
    OVERRIDE.0.fetch_or(topic.bit(), Ordering::Relaxed);
    if on {
        OVERRIDE.1.fetch_or(topic.bit(), Ordering::Relaxed);
    } else {
        OVERRIDE.1.fetch_and(!topic.bit(), Ordering::Relaxed);
    }
}

/// Whether lines of `topic` are written.
#[inline]
pub fn enabled(topic: Topic) -> bool {
    let bit = topic.bit();
    #[cfg(test)]
    if OVERRIDE.0.load(Ordering::Relaxed) & bit != 0 {
        return OVERRIDE.1.load(Ordering::Relaxed) & bit != 0;
    }
    match topic {
        Topic::Compile => PROGRESS.get().is_some(),
        _ => knob_bits() & bit != 0,
    }
}

type Sink = fn(&str);

static PROGRESS: OnceLock<Sink> = OnceLock::new();

/// Install the sink of the progress stream (`Compile`, `Coverage`): it
/// gets each line complete with its prefix. Only the first call counts.
pub fn install_progress(sink: Sink) {
    let _ = PROGRESS.set(sink);
}

/// The value of `TLX_IR_DUMP` (the name to dump; `*`: all), if set.
pub fn ir_dump() -> Option<String> {
    Env::process().string("TLX_IR_DUMP").filter(|s| !s.is_empty())
}

/// Whether `TLX_IR_DUMP_RAW` asks for the `Debug` form of IR dumps.
pub fn ir_dump_raw() -> bool {
    Env::process().flag("TLX_IR_DUMP_RAW")
}

/// Write `text` as a line of `topic` (`prefix: text`; a multi-line text
/// has the prefix on its first line only), if the topic is enabled.
pub fn emit(topic: Topic, text: impl Display) {
    if enabled(topic) {
        write(topic, &text.to_string());
    }
}

/// `emit` for text that is built only if the topic is enabled.
pub fn emit_with(topic: Topic, text: impl FnOnce() -> String) {
    if enabled(topic) {
        write(topic, &text());
    }
}

fn write(topic: Topic, text: &str) {
    let line = format!("{}: {text}", topic.prefix());
    if topic.progress() {
        if let Some(sink) = PROGRESS.get() {
            sink(&line);
        }
    } else {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_have_distinct_bits_and_declared_knobs() {
        for (i, t) in TOPICS.iter().enumerate() {
            assert_eq!(*t as usize, i);
            if let Some(k) = t.knob() {
                crate::knobs::decl(k);
            }
        }
    }

    #[test]
    fn lines_have_the_prefix_of_their_topic() {
        use std::sync::Mutex;
        static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
        install_progress(|l| LINES.lock().unwrap().push(l.to_string()));
        emit(Topic::Compile, format_args!("pre-run: {} states", 3));
        set_for_test(Topic::Coverage, false);
        emit(Topic::Coverage, "var x");
        set_for_test(Topic::Coverage, true);
        emit_with(Topic::Coverage, || "boxed 1 Foo".to_string());
        assert_eq!(*LINES.lock().unwrap(), ["compile: pre-run: 3 states", "compile: boxed 1 Foo"]);
    }
}
