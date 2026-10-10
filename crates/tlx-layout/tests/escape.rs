//! Stable slots retain their original encoding when a different value escapes.
use std::sync::Arc;
use tlx_layout::{escape::EscapeLayout, Layout, Shape};
use tlx_value::Value;

#[test]
fn escaped_and_inline_forms_have_the_same_canonical_key() {
    let layout = EscapeLayout::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 3 }])));
    let vals = [Value::Int(2)];
    let inline = layout.encode(&vals).unwrap();
    let escaped = layout.force_escape(&vals).unwrap();
    assert_ne!(inline, escaped);
    assert_eq!(layout.decode(&inline), vals);
    assert_eq!(layout.decode(&escaped), vals);
    assert_eq!(layout.key(&inline), layout.key(&escaped));
}

#[test]
fn outliers_never_change_old_slots_and_returning_values_reuse_their_key() {
    let layout = EscapeLayout::new(Arc::new(Layout::new(vec![Shape::Int { lo: 0, hi: 3 }, Shape::Bool])));
    let old = layout.encode(&[Value::Int(1), Value::Bool(true)]).unwrap();
    for n in [4, 100, i64::MAX] {
        let vals = [Value::Int(n), Value::Bool(false)];
        let escaped = layout.encode(&vals).unwrap();
        assert!(layout.is_escaped(&escaped));
        assert_eq!(layout.decode(&escaped), vals);
        assert_eq!(layout.key(&escaped), layout.key(&layout.force_escape(&vals).unwrap()));
        assert_eq!(layout.decode(&old), [Value::Int(1), Value::Bool(true)]);
    }
    assert_eq!(old, layout.encode(&[Value::Int(1), Value::Bool(true)]).unwrap());
    let mut queued = vec![0; layout.words()];
    layout.queue_words(&old, &mut queued);
    assert_eq!(layout.compact(&queued), old);
}

#[test]
fn invalid_escape_handles_are_rejected_before_decoding() {
    let layout = EscapeLayout::new(Arc::new(Layout::new(vec![Shape::Bool])));
    let mut bad = vec![0; layout.words()];
    bad[1] = u64::MAX;
    assert!(layout.validate(&bad).is_err());
}
