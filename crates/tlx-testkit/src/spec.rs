//! Loading specs from strings and running them in each execution tier.

use std::sync::Arc;

use tlx_eval::{Interp, InterpModel};
use tlx_runtime::{explore, Options, Outcome, Report};
use tlx_sema::hir::{self, DefKind};
use tlx_sema::Program;

use crate::{library_paths, module_name, TempDir};

pub fn interpreter(tag: &str, src: &str, cfg: &str) -> Arc<Interp> {
    let dir = TempDir::with_spec(tag, src, cfg);
    let path = dir.path().join(format!("{}.tla", module_name(src).unwrap()));
    let program = Program::load(&path, &[]).unwrap_or_else(|errors| panic!("load: {}", errors[0].message));
    let cfg = tlx_front::cfg::parse_cfg(cfg).unwrap();
    Interp::new(program, cfg).unwrap_or_else(|error| panic!("interp: {}", error.message))
}

#[cfg(test)]
mod tests {
    #[test]
    fn interpreter_loads_without_model_initialization() {
        let interp =
            super::interpreter("load", &crate::module("Load", "VARIABLE x\nInit == x = 0\nNext == x' = x"), "INIT Init\nNEXT Next\n");
        assert_eq!(interp.prog.vars.len(), 1);
    }

    #[test]
    fn operator_body_resolves_a_definition_with_its_parameters() {
        let src = crate::module("Lookup", "EXTENDS Naturals\nVARIABLE x\nInit == x = 0\nParam(a) == a + x\nNext == x' = x");
        let interp = super::interpreter("lookup", &src, "INIT Init\nNEXT Next\n");
        assert!(super::operator_body(&interp, "Init").1.is_empty());
        assert_eq!(super::operator_body(&interp, "Param").1.len(), 1);
    }

    #[test]
    #[should_panic(expected = "definition")]
    fn operator_body_panics_on_an_unknown_name() {
        let src = crate::module("Missing", "VARIABLE x\nInit == x = 0\nNext == x' = x");
        let interp = super::interpreter("missing", &src, "INIT Init\nNEXT Next\n");
        super::operator_body(&interp, "Nope");
    }
}

/// Load the module `src` (complete text, `---- MODULE Name ---- ... ====`)
/// with the configuration `cfg` into an interpreted model. Extra modules it
/// extends can be given in `extra` (complete texts). The community modules
/// of `third_party/` are on the search path when present.
pub fn try_load_with(src: &str, extra: &[&str], cfg: &str) -> Result<InterpModel, String> {
    let name = module_name(src).ok_or("no MODULE header")?;
    let dir = TempDir::new("spec");
    for m in extra {
        let n = module_name(m).ok_or("no MODULE header in an extra module")?;
        dir.write(&format!("{n}.tla"), m);
    }
    let tla = dir.write(&format!("{name}.tla"), src);
    let prog = Program::load(&tla, &library_paths()).map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))?;
    let cfg = tlx_front::cfg::parse_cfg(cfg).map_err(|e| format!("cfg: {e:?}"))?;
    Interp::new(prog, cfg).and_then(InterpModel::new).map_err(|e| e.into_message())
}

/// `try_load_with` without extra modules; panics on errors.
pub fn load(src: &str, cfg: &str) -> InterpModel {
    try_load_with(src, &[], cfg).unwrap_or_else(|e| panic!("loading the spec failed: {e}\n{src}"))
}

/// The resolved body of operator `name` in `ip` and its parameters. Panics
/// when there is no definition of that name or it is not a resolved operator;
/// a fixture looking a definition up by name wants exactly that.
pub fn operator_body<'a>(ip: &'a Interp, name: &str) -> (&'a hir::Expr, &'a [hir::LocalId]) {
    let d = ip.prog.defs.iter().find(|d| &*d.name == name).expect("definition");
    let DefKind::Op(e) = &d.kind else { panic!("{name} is not resolved") };
    (e, &d.params)
}

/// An execution tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The interpreter (default `tlx` mode).
    Interp,
    /// Encoded states, IR executor and native code (`-compile`). Uses the
    /// environment's `TLX_*` settings (e.g. `TLX_PARANOID`).
    #[cfg(feature = "compiled")]
    Compile,
}

/// The observable result of a model-checking run (what spectest compares,
/// plus the depth and the trace length).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// `Ok`, `Deadlock`, `InvariantViolated(name)`, ..., `Error(message)`.
    pub outcome: String,
    pub generated: u64,
    pub distinct: u64,
    pub depth: u32,
    pub trace_len: usize,
}

fn summary<S>(r: &Report<S>) -> Summary {
    let outcome = match &r.outcome {
        Outcome::Error(e) => format!("Error({})", e.message),
        o => format!("{o:?}"),
    };
    let s = &r.stats;
    Summary { outcome, generated: s.generated, distinct: s.distinct, depth: s.depth, trace_len: r.trace.len() }
}

/// Model-check `src`/`cfg` in `mode` with one worker.
pub fn run_mode(src: &str, cfg: &str, mode: Mode) -> Summary {
    let model = load(src, cfg);
    let check_deadlock = model.check_deadlock;
    model.ip.set_run_config(1, check_deadlock);
    let opts = Options { workers: 1, check_deadlock, ..Default::default() };
    match mode {
        Mode::Interp => summary(&explore(&model, &opts)),
        #[cfg(feature = "compiled")]
        Mode::Compile => {
            let (_, r) = tlx_compile::enc::check(Arc::new(model), &opts, 16).unwrap();
            summary(&r)
        }
    }
}

/// Model-check `src`/`cfg` in every tier and assert that the results agree;
/// returns the (common) result.
#[cfg(feature = "compiled")]
pub fn assert_modes_agree(src: &str, cfg: &str) -> Summary {
    let want = run_mode(src, cfg, Mode::Interp);
    let got = run_mode(src, cfg, Mode::Compile);
    assert_eq!(got, want, "Compile differs from the interpreter on\n{src}\n{cfg}");
    want
}
