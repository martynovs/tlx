use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tlx_eval::model::State;
use tlx_eval::{Interp, InterpModel};
use tlx_runtime::Model;
use tlx_sema::Program;
use tlx_value::Value;

/// Bounded BFS sampling. Loading/model setup and a single evaluation between
/// enumeration callbacks cannot be interrupted by the sampling time budget.
pub fn sample_states(tla: &Path, cfg: &Path, max_states: usize, time_budget: Duration) -> Result<Vec<Arc<[Value]>>, String> {
    let includes: Vec<PathBuf> = crate::library_paths();
    let prog = Program::load(tla, &includes).map_err(|es| format!("load: {}", es.first().map(|e| e.message.as_str()).unwrap_or("?")))?;
    let cfg_text = std::fs::read_to_string(cfg).map_err(|e| format!("cfg: {e}"))?;
    let cfg = tlx_front::cfg::parse_cfg(&cfg_text).map_err(|_| "cfg parse error".to_string())?;
    let model = Interp::new(prog, cfg).and_then(InterpModel::new).map_err(|e| format!("interp: {}", e.message))?;
    if !model.has_behavior {
        return Err("no behavior spec".into());
    }
    model.ip.set_run_config(1, false);
    sample_model(&model, max_states, time_budget)
}

fn sample_model(model: &InterpModel, max_states: usize, time_budget: Duration) -> Result<Vec<Arc<[Value]>>, String> {
    let start = Instant::now();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut states: Vec<Arc<[Value]>> = Vec::new();
    let mut queue: VecDeque<State> = VecDeque::new();
    let add = |s: State, seen: &mut HashSet<u64>, states: &mut Vec<Arc<[Value]>>, queue: &mut VecDeque<State>| {
        if states.len() >= max_states || start.elapsed() >= time_budget {
            return false;
        }
        if seen.insert(model.fingerprint(&s)) {
            states.push(s.vals.clone());
            queue.push_back(s);
        }
        states.len() < max_states && start.elapsed() < time_budget
    };
    if max_states == 0 || start.elapsed() >= time_budget {
        return Ok(states);
    }
    model.init_states_until(&mut |s| add(s, &mut seen, &mut states, &mut queue)).map_err(|e| format!("init: {}", e.message))?;
    while let Some(s) = queue.pop_front() {
        if states.len() >= max_states || start.elapsed() >= time_budget {
            break;
        }
        if !model.constraint(&s).map_err(|e| format!("constraint: {}", e.message))? {
            continue;
        }
        if start.elapsed() >= time_budget {
            break;
        }
        model.successors(&s, &mut |t, _| add(t, &mut seen, &mut states, &mut queue)).map_err(|e| format!("next: {}", e.message))?;
    }
    Ok(states)
}

pub type CorpusModel = (String, PathBuf, PathBuf);

/// The corpus models that TLC checks successfully whose id contains `filter`,
/// sorted by id. None (after `crate::skip`) when the external data
/// (`corpus/out/tlc.json`, `third_party/Examples`) is not installed; panics
/// when the data is present but the selection is empty, so that a run that
/// checks nothing cannot pass.
pub fn models(test: &str, filter: Option<&str>) -> Option<Vec<CorpusModel>> {
    let corpus = crate::require_corpus(test, "tlc.json")?;
    crate::require_third_party(test, "Examples")?;
    let text = std::fs::read_to_string(&corpus).unwrap_or_else(|error| panic!("{}: {error}", corpus.display()));
    let models = parse_models(&text, filter);
    check_selection(&models, filter).unwrap_or_else(|error| panic!("{test}: {error}"));
    Some(models)
}

fn check_selection(models: &[CorpusModel], filter: Option<&str>) -> Result<(), String> {
    match (models.is_empty(), filter) {
        (false, _) => Ok(()),
        (true, Some(filter)) => Err(format!("no corpus model matches the filter {filter:?}")),
        (true, None) => Err("the corpus manifest lists no successfully checked model".into()),
    }
}

/// Fails `test` unless at least one of the `total` selected models was
/// actually checked (`tested`): per-model skips are allowed, a run where
/// every model was skipped is not.
pub fn require_tested(test: &str, total: usize, tested: usize) {
    check_coverage(total, tested).unwrap_or_else(|error| panic!("{test}: {error}"));
}

fn check_coverage(total: usize, tested: usize) -> Result<(), String> {
    match (total, tested) {
        (0, _) => Err("no model was run".into()),
        (_, 0) => Err(format!("all {total} models were skipped, none was tested")),
        _ => Ok(()),
    }
}

/// The number of models to check in parallel: `$var` if set (a positive
/// integer, else panic), otherwise `default`.
pub fn workers_from_env(var: &str, default: usize) -> usize {
    match std::env::var(var) {
        Ok(text) => parse_workers(var, &text).unwrap_or_else(|error| panic!("{error}")),
        Err(_) => default,
    }
}

fn parse_workers(var: &str, text: &str) -> Result<usize, String> {
    match text.trim().parse::<usize>() {
        Ok(0) | Err(_) => Err(format!("{var}={text:?}: expected a positive number of workers")),
        Ok(workers) => Ok(workers),
    }
}

fn parse_models(text: &str, filter: Option<&str>) -> Vec<CorpusModel> {
    let json: serde_json::Value = serde_json::from_str(text).unwrap();
    let mut models: Vec<_> = json
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, entry)| entry["result"] == "success")
        .filter(|(id, _)| filter.is_none_or(|filter| id.contains(filter)))
        .filter_map(|(id, entry)| {
            Some((id.clone(), crate::rebase_third_party(entry["tla"].as_str()?), crate::rebase_third_party(entry["cfg"].as_str()?)))
        })
        .collect();
    models.sort();
    models
}

pub fn map_models<Result: Send>(
    models: &[CorpusModel],
    workers: usize,
    check: impl Fn(&Path, &Path) -> Result + Sync,
) -> Vec<(String, Result)> {
    assert!(workers > 0, "map_models: zero workers would check none of {} models", models.len());
    let results = Mutex::new(Vec::new());
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((id, tla, cfg)) = models.get(index) else { break };
                let result = check(tla, cfg);
                results.lock().unwrap().push((id.clone(), result));
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by(|left, right| left.0.cmp(&right.0));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_filter_and_parallel_order() {
        let text = r#"{"z":{"result":"success","tla":"z.tla","cfg":"z.cfg"},"a":{"result":"success","tla":"a.tla","cfg":"a.cfg"},"bad":{"result":"error"},"missing":{"result":"success"}}"#;
        let models = parse_models(text, None);
        assert_eq!(models.iter().map(|model| model.0.as_str()).collect::<Vec<_>>(), ["a", "z"]);
        assert_eq!(parse_models(text, Some("z")).len(), 1);
        assert_eq!(map_models(&models, 1, |tla, _| tla.to_path_buf()), map_models(&models, 3, |tla, _| tla.to_path_buf()));
    }

    #[test]
    fn empty_selection_is_an_error() {
        let text = r#"{"a":{"result":"success","tla":"a.tla","cfg":"a.cfg"},"bad":{"result":"error"}}"#;
        assert!(check_selection(&parse_models(text, Some("a")), Some("a")).is_ok());
        let error = check_selection(&parse_models(text, Some("nomatch")), Some("nomatch")).unwrap_err();
        assert!(error.contains("\"nomatch\""), "{error}");
        assert!(check_selection(&parse_models(r#"{"bad":{"result":"error"}}"#, None), None).is_err());
    }

    #[test]
    fn coverage_needs_a_tested_model() {
        assert!(check_coverage(0, 0).is_err());
        assert_eq!(check_coverage(3, 0).unwrap_err(), "all 3 models were skipped, none was tested");
        assert!(check_coverage(3, 1).is_ok());
        require_tested("synthetic", 2, 2);
    }

    #[test]
    #[should_panic(expected = "synthetic: all 2 models were skipped")]
    fn all_skipped_fails() {
        require_tested("synthetic", 2, 0);
    }

    #[test]
    #[should_panic(expected = "zero workers")]
    fn zero_workers_fails() {
        let models = vec![("a".to_string(), PathBuf::from("a.tla"), PathBuf::from("a.cfg"))];
        map_models(&models, 0, |_, _| ());
    }

    #[test]
    fn worker_counts() {
        assert_eq!(parse_workers("JOBS", "4"), Ok(4));
        assert!(parse_workers("JOBS", "0").is_err());
        assert!(parse_workers("JOBS", "many").is_err());
    }

    #[test]
    fn sampling_is_bounded_bfs() {
        let dir = crate::TempDir::with_spec(
            "sample",
            &crate::module("Sample", "EXTENDS Naturals\nVARIABLE x\nInit == x = 0\nNext == x < 4 /\\ x' = x + 1"),
            "INIT Init\nNEXT Next\n",
        );
        let states = sample_states(&dir.path().join("Sample.tla"), &dir.path().join("Sample.cfg"), 3, Duration::from_secs(5)).unwrap();
        assert_eq!(
            states.iter().map(|state| state.to_vec()).collect::<Vec<_>>(),
            vec![vec![Value::Int(0)], vec![Value::Int(1)], vec![Value::Int(2)]]
        );
    }

    #[test]
    fn sampling_stops_before_later_errors() {
        for (init, next, cap) in [
            ("\\E n \\in 1..10: x = IF n < 4 THEN n ELSE 1 \\div 0", "x' = x", 3),
            ("x = 0", "\\E n \\in 1..10: x' = IF n < 3 THEN n ELSE 1 \\div 0", 3),
        ] {
            let dir = crate::TempDir::with_spec(
                "sample-cap",
                &crate::module("SampleCap", &format!("EXTENDS Naturals\nVARIABLE x\nInit == {init}\nNext == {next}")),
                "INIT Init\nNEXT Next\n",
            );
            let tla = dir.path().join("SampleCap.tla");
            let cfg = dir.path().join("SampleCap.cfg");
            assert_eq!(sample_states(&tla, &cfg, cap, Duration::from_secs(5)).unwrap().len(), cap);
            assert!(sample_states(&tla, &cfg, 10, Duration::from_secs(5)).is_err());
            assert!(sample_states(&tla, &cfg, 10, Duration::ZERO).unwrap().is_empty());
            assert!(sample_states(&tla, &cfg, 0, Duration::from_secs(5)).unwrap().is_empty());
        }
    }

    #[test]
    fn flood_admission_and_constraints() {
        for (init, next, count) in [("\\E n \\in 1..100: Tick /\\ x = n", "x' = x", 3), ("x = 0", "\\E n \\in 1..100: Tick /\\ x' = n", 2)]
        {
            let m = crate::spec::load(
                &crate::module("Flood", &format!("EXTENDS Naturals, TLC\nVARIABLE x\nASSUME TLCSet(0, 0)\nTick == TLCSet(0, TLCGet(0) + 1)\nCount == TLCGet(0)\nInit == {init}\nNext == {next}")),
                "INIT Init\nNEXT Next\n",
            );
            m.check_assumes().unwrap();
            let states = sample_model(&m, 3, Duration::from_secs(5)).unwrap();
            assert_eq!(states.len(), 3);
            let d = m.ip.prog.defs.iter().position(|d| &*d.name == "Count").unwrap();
            assert_eq!(m.ip.eval_def_in_state(tlx_sema::hir::DefId(d as u32), &states[0]).unwrap(), Value::Int(count));
        }
        let src = crate::module("Constraint", "EXTENDS Naturals\nVARIABLE x\nInit == x = 0\nNext == x' = x + 1\nC == x < 1\nV == x - x");
        let m = crate::spec::load(&src, "INIT Init\nNEXT Next\nCONSTRAINT C\n");
        assert_eq!(sample_model(&m, 10, Duration::from_secs(5)).unwrap(), vec![Arc::from([Value::Int(0)]), Arc::from([Value::Int(1)])]);
        let m = crate::spec::load(&src, "INIT Init\nNEXT Next\nVIEW V\n");
        assert_eq!(sample_model(&m, 10, Duration::from_secs(5)).unwrap(), vec![Arc::from([Value::Int(0)])]);
    }
}
