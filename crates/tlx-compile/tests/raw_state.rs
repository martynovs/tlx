//! Safe codec boundaries must not manufacture process-local symbol handles.
use std::sync::Arc;
use tlx_compile::enc::EncModel;
use tlx_compile::settings::Settings;
use tlx_layout::{Layout, Shape};
#[cfg(debug_assertions)]
use tlx_runtime::Model;
use tlx_runtime::StateCodec;
use tlx_value::Value;

fn model(shape: Shape) -> EncModel {
    let base = tlx_testkit::spec::load(
        "---- MODULE RawState ----\nEXTENDS Naturals\nVARIABLE x\nInit == x = 0\nNext == UNCHANGED x\n====\n",
        "INIT Init\nNEXT Next\n",
    );
    let settings = Settings::from_lookup(|k| (k == "TLX_NO_NATIVE").then(|| "1".to_string())).unwrap();
    EncModel::new(Arc::new(base), Arc::new(Layout::new(vec![shape])), vec![], &settings)
}

#[test]
fn safe_state_codec_rejects_forged_dyn_words() {
    let m = model(Shape::Dyn);
    let bad = ((u32::MAX as u64) << 3) | 1;
    assert!(m.validate(&[bad]).is_err());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| StateCodec::decode(&m, &[bad], 1, 0))).is_err());
    let good = tlx_layout::dynval::intern(&Value::str("raw-state"));
    let s = StateCodec::decode(&m, &[good], 1, 0);
    assert_eq!(&*m.values(&s), &[Value::str("raw-state")]);
    assert_eq!(s.words(), &[good]);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| StateCodec::decode_into(&m, &[bad], 1, 0, s))).is_err());
}

#[cfg(debug_assertions)]
#[test]
fn debug_rejects_states_from_another_model() {
    let a = model(Shape::Int { lo: 0, hi: i64::MAX });
    let b = model(Shape::Dyn);
    let s = StateCodec::decode(&a, &[((u32::MAX as u64) << 3) | 1], 1, 0);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.render(&s))).is_err());
}
