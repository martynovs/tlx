//! Front end of the opt-in bounded symmetry diagnostic.
use super::*;
use tlx_eval::model::autosym::{self, Verdict};
use tlx_eval::model::symcheck::{self, Bounds, Edge};

pub(super) fn run(model: &InterpModel, bounds: &Bounds, started: Instant) -> ExitCode {
    out::info(code::GENERAL, "Checking declared SYMMETRY without symmetry or VIEW reduction (interpreter).");
    let exit = match symcheck::check(model, bounds) {
        Err(e) => {
            out::error(code::GENERAL, &format!("Symmetry check incomplete: {e}"));
            2
        }
        Ok(r) => {
            out::info(
                code::GENERAL,
                &format!(
                    "Symmetry check: {} states checked, {} permutations, depth {} (bounds: {} states, depth {}).",
                    r.checked, r.permutations, r.depth, bounds.states, bounds.depth
                ),
            );
            if let Some(w) = r.witness {
                out::error(code::GENERAL, &format!("SYMMETRY violation: {}.\nPermutation: {}", w.part, w.permutation));
                out::error(code::TLC_BEHAVIOR_UP_TO_THIS_POINT, "The behavior up to this point is:");
                for (i, (s, a)) in w.trace.iter().enumerate() {
                    let label = a.map_or_else(|| model.init_label(), |a| model.action_label(a as usize));
                    print!("{}", out::state(i + 1, &format!("<{label}>"), &model.render(s)));
                }
                out::info(code::GENERAL, &format!("Permuted state p(s):\n{}", model.render(&w.image)));
                match w.edge {
                    Some(Edge::Images { original, image }) => {
                        out::info(
                            code::GENERAL,
                            &format!(
                                "Original successor t:\n{}\nPermuted successor p(t):\n{}",
                                model.render(&original),
                                model.render(&image)
                            ),
                        );
                    }
                    Some(Edge::ExtraSuccessor(t)) => {
                        out::info(code::GENERAL, &format!("Extra successor of p(s), absent from p(Next(s)):\n{}", model.render(&t)));
                    }
                    None => {}
                }
                1
            } else {
                out::info(
                    code::GENERAL,
                    &format!(
                        "No symmetry violation found within bounds ({}). This is not a proof of symmetry.",
                        if r.limited { "search bound reached" } else { "reachable queue exhausted" }
                    ),
                );
                0
            }
        }
    };
    print_finished(started);
    ExitCode::from(exit)
}

/// `-checksymmetry -suggest`: the static analysis of every candidate set of
/// model values, and a bounded check of each (of the proven group, as a
/// cross-check of the proof, or of all permutations of a set the analysis
/// does not prove). A proven group with a witness is an internal error
/// (exit 101).
pub(super) fn suggest(model: &InterpModel, bounds: &Bounds, started: Instant) -> ExitCode {
    out::info(code::GENERAL, "Checking the sets of model values for symmetry: static proof and bounded check (interpreter).");
    let an = autosym::analyze(model);
    let mut exit = 0;
    let mut proven = Vec::new();
    let mut bounded = Vec::new();
    for s in &an.sets {
        let c = &s.candidate;
        let elems: Vec<String> = c.elems.iter().map(|e| e.to_string()).collect();
        let head = format!("Symmetry: {} = {{{}}}", c.name, elems.join(", "));
        let perms = match &s.verdict {
            Verdict::Declared | Verdict::Unused => {
                out::info(code::GENERAL, &format!("{head}: {}.", s.reason().unwrap_or_default()));
                continue;
            }
            Verdict::Proven { .. } => s.generators(),
            _ => autosym::transpositions(&c.elems),
        };
        let what = match s.group_text() {
            Some(g) => format!("proven: {g}"),
            None => s.reason().unwrap_or_default(),
        };
        let check = match symcheck::check_perms(model, perms, bounds) {
            Err(e) => format!("bounded check incomplete: {e}"),
            Ok(r) => match r.witness {
                Some(w) => {
                    if s.group_text().is_some() {
                        exit = 101;
                    }
                    format!("bounded check: violated ({}; permutation {})", w.part, w.permutation)
                }
                None => {
                    match s.group_text() {
                        Some(g) => proven.push(g),
                        None => bounded.push(format!("Permutations({})", c.name)),
                    }
                    format!("bounded check: no violation in {} states, depth {}", r.checked, r.depth)
                }
            },
        };
        out::info(code::GENERAL, &format!("{head}: {what}; {check}."));
    }
    if exit == 101 {
        out::error(code::GENERAL, "A proven symmetry has a witness: the static analysis is wrong (an internal error of tlx).");
    }
    if let Some(def) = an.symmetry_definition() {
        out::info(code::GENERAL, &format!("Proven symmetry: define Perms == {def} and add \"SYMMETRY Perms\" to the cfg."));
    } else if !proven.is_empty() {
        out::info(code::GENERAL, &format!("Proven symmetry: {}.", proven.join(" x ")));
    }
    if !bounded.is_empty() {
        out::info(
            code::GENERAL,
            &format!(
                "Not proven, no violation within bounds (not a proof; use only if the symmetry is known to hold): {}.",
                bounded.join(" \\cup ")
            ),
        );
    }
    out::info(code::GENERAL, &format!("Analysis time: {:.3}s.", an.elapsed.as_secs_f64()));
    print_finished(started);
    ExitCode::from(exit)
}
