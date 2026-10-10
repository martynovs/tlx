//! An allocated symbol segment must not make its unwritten slots valid.
use tlx_layout::{dynval, LayoutNode, Shape};
use tlx_value::{intern, Sym};

#[test]
fn checked_raw_handle_rejects_unwritten_slot_in_allocated_segment() {
    Sym::new("allocated-segment-0");
    let mut next = intern::len();
    // A segment starts at 2^k - 1. Leave next inside the last allocated
    // segment, rather than at the beginning of an unallocated one.
    while (next + 1).is_power_of_two() {
        Sym::new(&format!("allocated-segment-{next}"));
        next = intern::len();
    }
    assert!(Sym::try_from_id(next as u32).is_none());
    for tag in [1, 2] {
        let raw = (next as u64) << 3 | tag;
        assert!(dynval::try_lookup(raw).is_none());
        assert!(LayoutNode::new(&Shape::Dyn).validate_words(&[raw]).is_err());
    }
}
