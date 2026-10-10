//! The permutation walk behind `Permutations` and `SetToSeqs`
//! (`SequencesExt.java`): the index-order DFS and its two leaves.

use tlx_value::Value;

use crate::error::EResult;

/// A depth-first walk over the permutations of `elems`, in the fixed visit
/// order "smallest unused index first", calling `leaf` once per complete
/// permutation. `cur` holds the prefix being built, `used` the indices
/// already taken. A leaf error stops the walk and is returned unchanged, so
/// the first error wins exactly as in TLC's enumeration.
fn permute_walk(
    elems: &[Value],
    cur: &mut Vec<Value>,
    used: &mut Vec<bool>,
    leaf: &mut impl FnMut(&[Value]) -> EResult<()>,
) -> EResult<()> {
    if cur.len() == elems.len() {
        return leaf(cur);
    }
    for i in 0..elems.len() {
        if !used[i] {
            used[i] = true;
            cur.push(elems[i].clone());
            permute_walk(elems, cur, used, leaf)?;
            cur.pop();
            used[i] = false;
        }
    }
    Ok(())
}

/// Every permutation of `elems`, in `permute_walk`'s visit order, handed to
/// `leaf` as the completed permutation (`SetToSeqs` and `Permutations`
/// differ only in the leaf).
pub(super) fn for_each_permutation(elems: &[Value], mut leaf: impl FnMut(&[Value]) -> EResult<()>) -> EResult<()> {
    permute_walk(elems, &mut Vec::new(), &mut vec![false; elems.len()], &mut leaf)
}

/// The function value of the permutation `cur` of `elems`: the bijection
/// `elems[i] -> cur[i]` (the `Permutations` leaf).
pub(super) fn function_permutation(elems: &[Value], cur: &[Value]) -> EResult<Value> {
    let pairs = elems.iter().cloned().zip(cur.iter().cloned()).collect();
    Ok(Value::fcn_rcd_from_pairs(pairs)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EvalError;

    /// All permutations of `0..n` in lexicographic order, built by inserting
    /// each index into every position of the previous permutations (a
    /// different construction than the index-order DFS under test), and then
    /// sorted. A walk that always takes its smallest unused index emits an
    /// index sequence per prefix in exactly that order.
    fn reference(n: usize) -> Vec<Vec<usize>> {
        let mut perms: Vec<Vec<usize>> = vec![Vec::new()];
        for i in 0..n {
            let mut next = Vec::new();
            for p in &perms {
                for pos in 0..=p.len() {
                    let mut q = p.clone();
                    q.insert(pos, i);
                    next.push(q);
                }
            }
            perms = next;
        }
        perms.sort();
        perms
    }

    /// `0..n` as the values the two built-ins permute.
    fn elems(n: usize) -> Vec<Value> {
        (0..n as i64).map(Value::Int).collect()
    }

    /// The integers of a visited permutation.
    fn ints(seq: &[Value]) -> Vec<i64> {
        seq.iter()
            .map(|v| match v {
                Value::Int(i) => *i,
                other => panic!("permutation element is not an integer: {other}"),
            })
            .collect()
    }

    /// The permutations `elems(n)` hands to its leaf, in visit order.
    fn visits(n: usize) -> Vec<Vec<i64>> {
        let mut out = Vec::new();
        for_each_permutation(&elems(n), |cur| {
            out.push(ints(cur));
            Ok(())
        })
        .expect("an infallible leaf cannot fail");
        out
    }

    /// The `SetToSeqs` leaf values of `elems(n)`, in visit order.
    fn seqs(n: usize) -> Vec<Value> {
        let mut out = Vec::new();
        for_each_permutation(&elems(n), |cur| {
            out.push(Value::tuple(cur.to_vec()));
            Ok(())
        })
        .expect("an infallible leaf cannot fail");
        out
    }

    /// The `Permutations` leaf values of `elems(n)`, in visit order.
    fn fcns(n: usize) -> Vec<Value> {
        let es = elems(n);
        let mut out = Vec::new();
        for_each_permutation(&es, |cur| {
            out.push(function_permutation(&es, cur)?);
            Ok(())
        })
        .expect("integer permutations build functions");
        out
    }

    #[test]
    fn visit_order_is_the_tlc_enumeration_order_for_n_0_to_6() {
        for n in 0..=6 {
            let want: Vec<Vec<i64>> = reference(n).iter().map(|p| p.iter().map(|i| *i as i64).collect()).collect();
            assert_eq!(visits(n), want, "n = {n}");
        }
    }

    #[test]
    fn visit_order_of_three_elements_is_lexicographic() {
        assert_eq!(visits(3), vec![vec![0, 1, 2], vec![0, 2, 1], vec![1, 0, 2], vec![1, 2, 0], vec![2, 0, 1], vec![2, 1, 0]]);
    }

    #[test]
    fn counts_are_n_factorial() {
        let mut fact = 1usize;
        for n in 0..=6 {
            if n > 0 {
                fact *= n;
            }
            assert_eq!(visits(n).len(), fact, "visits n = {n}");
            assert_eq!(seqs(n).len(), fact, "SetToSeqs n = {n}");
            assert_eq!(fcns(n).len(), fact, "Permutations n = {n}");
        }
    }

    #[test]
    fn set_to_seqs_leaves_are_the_visited_sequences() {
        for n in 0..=6 {
            let want: Vec<Value> = visits(n).iter().map(|p| Value::tuple(p.iter().map(|i| Value::Int(*i)).collect())).collect();
            assert_eq!(seqs(n), want, "n = {n}");
        }
    }

    #[test]
    fn permutations_leaves_map_elements_onto_themselves_in_visit_order() {
        for n in 0..=6 {
            let es = elems(n);
            // Build the expected functions from the reference permutations
            // with the value API, not through the leaf helper.
            let want: Vec<Value> = reference(n)
                .iter()
                .map(|p| {
                    let cur: Vec<Value> = p.iter().map(|i| Value::Int(*i as i64)).collect();
                    let pairs = es.iter().cloned().zip(cur.into_iter()).collect();
                    Value::fcn_rcd_from_pairs(pairs).expect("distinct integer keys")
                })
                .collect();
            assert_eq!(fcns(n), want, "n = {n}");
        }
    }

    #[test]
    fn permutations_are_distinct_functions() {
        let f = fcns(5);
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j], "bijections {i} and {j} coincide");
            }
        }
    }

    #[test]
    fn empty_input_visits_the_empty_permutation_once() {
        assert_eq!(visits(0), vec![Vec::<i64>::new()]);
        assert_eq!(seqs(0), vec![Value::tuple(Vec::new())]);
        let empty = fcns(0);
        assert_eq!(empty.len(), 1);
        assert!(empty[0].is_empty_fcn(), "the empty permutation is the empty function");
    }

    #[test]
    fn leaf_error_stops_the_walk_at_the_failing_permutation() {
        let es = elems(3);
        let mut calls = 0usize;
        let err = for_each_permutation(&es, |_cur| {
            calls += 1;
            if calls == 3 {
                Err(EvalError::new("leaf failed"))
            } else {
                Ok(())
            }
        })
        .expect_err("the third leaf fails");
        assert_eq!(calls, 3, "the walk stops at the failing leaf");
        assert_eq!(err.to_string(), "leaf failed", "the leaf error is returned unchanged");
    }

    #[test]
    fn leaf_error_on_the_first_permutation_stops_before_any_other() {
        let es = elems(3);
        let mut calls = 0usize;
        let err = for_each_permutation(&es, |cur| {
            calls += 1;
            assert_eq!(ints(cur), vec![0, 1, 2], "the first permutation is visited first");
            Err(EvalError::new("boom"))
        })
        .expect_err("the first leaf fails");
        assert_eq!(calls, 1);
        assert_eq!(err.to_string(), "boom");
    }

    #[test]
    fn leaf_error_from_a_deep_frame_propagates() {
        let es = elems(3);
        let mut calls = 0usize;
        let err = for_each_permutation(&es, |cur| {
            calls += 1;
            if ints(cur) == vec![2, 1, 0] {
                Err(EvalError::new("last"))
            } else {
                Ok(())
            }
        })
        .expect_err("the last leaf fails");
        assert_eq!(calls, 6, "every earlier permutation was visited");
        assert_eq!(err.to_string(), "last");
    }
}
