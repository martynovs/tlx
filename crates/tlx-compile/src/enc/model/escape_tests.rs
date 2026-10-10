use std::sync::Arc;

use super::*;
use tlx_testkit::spec::load;

#[test]
fn fingerprints_match_inline_and_escaped_forms() {
    let src = "---- MODULE EscapeFp ----\nEXTENDS Integers\nVARIABLE x\nInit == x = 0\nNext == UNCHANGED x\n====";
    let base = Arc::new(load(src, "INIT Init\nNEXT Next\n"));
    let settings = Settings::from_lookup(|k| match k {
        "TLX_ESCAPE" | "TLX_NO_NATIVE" => Some("1".into()),
        _ => None,
    })
    .unwrap();
    let model = EncModel::new(base, Arc::new(Layout::new(vec![Shape::Int { lo: -2, hi: 2 }])), Vec::new(), &settings);
    let codec = &model.escape.as_ref().unwrap().codec;
    for x in [-2, 0, 2, 3, i64::MAX] {
        let vals = [Value::Int(x)];
        let inline = codec.encode(&vals).unwrap();
        let escaped = codec.force_escape(&vals).unwrap();
        assert_eq!(codec.decode(&inline), codec.decode(&escaped));
        assert_eq!(model.words_fp(&inline, 1).unwrap(), model.words_fp(&escaped, 1).unwrap());
    }
}

#[test]
fn collection_and_view_fingerprints_match_both_physical_forms() {
    let src = "---- MODULE EscapeFp ----\nEXTENDS Integers, Sequences\nVARIABLE x\nInit == x = <<1, 2>>\nNext == UNCHANGED x\nV == x\n====";
    let settings = Settings::from_lookup(|k| match k {
        "TLX_ESCAPE" | "TLX_NO_NATIVE" => Some("1".into()),
        _ => None,
    })
    .unwrap();
    let cases = [
        Value::tuple(vec![Value::Int(1), Value::Int(2)]),
        Value::set_from(vec![Value::Int(1), Value::Int(2)]).unwrap(),
        Value::fcn_rcd_from_pairs(vec![(Value::Int(1), Value::Int(3)), (Value::Int(2), Value::Int(4))]).unwrap(),
    ];
    for value in cases {
        for view in ["", "VIEW V\n"] {
            let base = Arc::new(load(src, &format!("INIT Init\nNEXT Next\n{view}")));
            let defs = super::super::speculate::view_def(&base).map(|d| vec![(d, Shape::of(&value))]).unwrap_or_default();
            let model = EncModel::new(base, Arc::new(Layout::new(vec![Shape::of(&value)])), defs, &settings);
            let codec = &model.escape.as_ref().unwrap().codec;
            let vals = [value.clone()];
            let inline = codec.encode(&vals).unwrap();
            let escaped = codec.force_escape(&vals).unwrap();
            assert_eq!(codec.decode(&inline), codec.decode(&escaped));
            let state = model.encode(&VState::new(vals.to_vec().into(), 1, 0)).unwrap();
            assert_eq!(state.fp, model.words_fp(&escaped, 1).unwrap(), "value {value:?}, config {view:?}");
        }
    }
}

#[test]
fn print_only_actions_deoptimize_without_repeating_output() {
    let src = "---- MODULE EscapePrint ----\nEXTENDS Naturals, TLC\nVARIABLE x\nInit == x = 0\nNext == PrintT(x) /\\ x' = x + 1\n====";
    let base = Arc::new(load(src, "INIT Init\nNEXT Next\n"));
    let settings = Settings::from_lookup(|k| match k {
        "TLX_ESCAPE" | "TLX_NATIVE_SYNC" => Some("1".into()),
        "TLX_JIT" => Some("0".into()),
        _ => None,
    })
    .unwrap();
    let model = EncModel::new(base, Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 0 }])), Vec::new(), &settings);
    assert!(model.prints[0]);
    assert!(model.compiled_path(0, true).is_some());
    let state = model.encode(&VState::new(vec![Value::Int(0)].into(), 1, 0)).unwrap();
    let mut next = Vec::new();
    let (result, lines) = tlx_eval::Interp::hold_output(|| {
        model.action_successors(&state, 0, &mut |s, _| {
            next.push(model.values(&s));
            true
        })
    });
    assert!(result.unwrap());
    assert_eq!(next, vec![Arc::<[Value]>::from(vec![Value::Int(1)])]);
    assert_eq!(lines, vec!["0"]);
    assert!(model.escape.as_ref().unwrap().deopts.get() > 0, "printing alone must not force the action into the interpreter");
}
