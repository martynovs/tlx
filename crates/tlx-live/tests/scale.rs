//! Scale smoke test: a large graph must be checked in reasonable time.
//! `TLX_LIVE_SCALE=<states>` overrides the size (default 200_000).

mod common;

use common::*;
use std::time::Instant;
use tlx_live::{check, check_lasso, Bits, Graph, Ltl};

/// A ring of `n` states, each with a stuttering step, a step forward (A) and
/// two pseudo-random jumps (J). State atom 0 = p (every 1000th state),
/// 1 = ENABLED A (everywhere). Action atom 0 = A, 1 = J.
fn ring(n: u32) -> Graph {
    let mut g = Graph { init: vec![0], state_bits: Vec::new(), edges: Vec::with_capacity(n as usize) };
    let mut x = 0x1234_5678_9abc_def0u64;
    for s in 0..n {
        let p = u64::from(s % 1000 == 0);
        g.state_bits.push(Bits::from_u64(p | 0b10));
        let mut es = vec![(s, Bits::new()), ((s + 1) % n, Bits::from_u64(0b01))];
        for _ in 0..2 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            es.push(((x % u64::from(n)) as u32, Bits::from_u64(0b10)));
        }
        g.edges.push(es);
    }
    g
}

#[test]
fn large_ring() {
    // Miri runs ~1000x slower; shrink the ring but keep it large enough that
    // the product exploration and SCC handling are exercised.
    let default_n = if cfg!(miri) { 1_000 } else { 200_000 };
    let n = std::env::var("TLX_LIVE_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(default_n);
    let g = ring(n);
    let t = Instant::now();
    // Jumps can avoid p forever, so []<>p is violated even under WF(A).
    let prop = always(eventually(st(0)));
    let l = check_lasso(&g, &prop, &[wf(1, 0)]).expect_err("jumps can avoid p");
    assert!(lasso_error(&g, &prop, &[wf(1, 0)], &l).is_none());
    let t1 = t.elapsed();
    // Passing checks explore the whole product.
    let t = Instant::now();
    assert!(check(&g, &leads_to(st(1), st(1)), &[wf(1, 0)]).is_ok());
    // Under WF(A) every behavior takes A infinitely often.
    assert!(check(&g, &always(eventually(act(0))), &[wf(1, 0)]).is_ok());
    let t2 = t.elapsed();
    eprintln!("{n} states: violation {t1:?}, two passing checks {t2:?}");
}

#[test]
fn clock_settles_uses_linear_product() {
    use tlx_live::check_lasso_within;
    for n in [4, 8, 12, 16] {
        let g = graph(&[0], &(0..n).map(|i| 1 << i).collect::<Vec<_>>(), &(0..n).map(|i| (i, (i + 1) % n, 0)).collect::<Vec<_>>(), true);
        let prop = Ltl::Or((0..n).map(|i| eventually(always(st(i)))).collect());
        let mut peak = 0;
        let result = check_lasso_within(&g, &prop, &[], &mut |nodes, _, _| {
            peak = peak.max(nodes);
            nodes <= n as usize * 2
        });
        let lasso = result.expect("recurrence must not build an exponential product").expect_err("clock does not settle");
        assert!(lasso_error(&g, &prop, &[], &lasso).is_none());
        assert!(peak <= n as usize * 2);
    }
}

#[test]
fn budget_can_stop_before_product_construction() {
    use tlx_live::check_lasso_within;
    let g = graph(&[0], &[0], &[], true);
    let prop = Ltl::Or((0..8).map(|i| always(eventually(st(i)))).collect());
    let mut calls = 0;
    let result = check_lasso_within(&g, &prop, &[], &mut |nodes, edges, bytes| {
        calls += 1;
        assert_eq!((nodes, edges), (0, 0), "guard must run before tableau expansion");
        assert!(bytes > 0, "the formula interner is budgeted");
        false
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);
}

#[test]
fn temporary_tableau_branches_are_budgeted() {
    use tlx_live::check_lasso_within;
    let g = graph(&[0], &[0], &[], true);
    // The negation is a conjunction of 16 eventualities (not conditions on
    // the cycle: a tableau of 2^16 states). Stop during partial expansion.
    let prop = or((0..16).map(|i| always(st(i))).collect());
    let mut calls = 0;
    let result = check_lasso_within(&g, &prop, &[], &mut |nodes, edges, bytes| {
        calls += 1;
        assert_eq!((nodes, edges), (0, 0));
        bytes < 128 * 1024
    });
    assert!(result.is_err());
    assert!(calls > 1);
}

#[test]
fn waiting_states_are_not_built() {
    use tlx_live::check_lasso_within;
    // A ring of n states with atoms p_i (state % 100 == i) and q_i (state
    // % 100 not in i..i + 5) for 8 properties p_i ~> q_i: their negations
    // each wait in a state without literals, which would copy the ring 8
    // times (16 with the nested `<>` of `~>` under `[]`).
    let n = 20_000u32;
    let k = 8;
    let states: Vec<u64> = (0..n)
        .map(|s| {
            (0..k).fold(0, |b, i| b | (u64::from(s % 100 == i) << (2 * i)) | (u64::from(!(i..i + 5).contains(&(s % 100))) << (2 * i + 1)))
        })
        .collect();
    let edges: Vec<(u32, u32, u64)> = (0..n).map(|s| (s, (s + 1) % n, 1)).collect();
    let g = graph(&[0], &states, &edges, true);
    let prop = and((0..k).map(|i| always(leads_to(st(2 * i), st(2 * i + 1)))).collect());
    // The ring always moves on (WF of action 0): every q_i comes.
    let fair = [wf(2 * k, 0)];
    let g = {
        let mut g = g;
        for b in &mut g.state_bits {
            b.set(2 * k, true);
        }
        g
    };
    let mut peak = 0;
    let r = check_lasso_within(&g, &prop, &fair, &mut |nodes, _, _| {
        peak = peak.max(nodes);
        true
    });
    assert!(matches!(r, Ok(Ok(()))));
    // Only the states where some p_i holds and q_i does not yet (the
    // entries), and then until q_i (the commitments): far fewer than n.
    assert!(peak < n as usize, "{peak} product nodes for {n} states");
    // Without fairness the ring may stutter where q_i fails: a violation.
    let l = tlx_live::check_lasso(&g, &prop, &[]).expect_err("stuttering before q_i");
    assert!(lasso_error(&g, &prop, &[], &l).is_none());
}

#[test]
fn parts_over_the_budget_together_run_one_at_a_time() {
    use tlx_live::{check_lasso_csr_stats, CheckStats, Csr};
    // 8 properties p_i ~> q_i on a ring (as above), without fairness: a
    // violation in every part.
    let n = 20_000u32;
    let k = 8u32;
    let states: Vec<u64> = (0..n)
        .map(|s| {
            (0..k).fold(0, |b, i| b | (u64::from(s % 100 == i) << (2 * i)) | (u64::from(!(i..i + 50).contains(&(s % 100))) << (2 * i + 1)))
        })
        .collect();
    let edges: Vec<(u32, u32, u64)> = (0..n).map(|s| (s, (s + 1) % n, 1)).collect();
    let c = Csr::from_graph(&graph(&[0], &states, &edges, true));
    let prop = and((0..k).map(|i| always(leads_to(st(2 * i), st(2 * i + 1)))).collect());
    let run = |threads, limit: usize| {
        let (mut st, mut peak, mut refused) = (CheckStats::default(), 0, false);
        let r = check_lasso_csr_stats(
            &c,
            &prop,
            &[],
            threads,
            &mut |_, _, bytes| {
                peak = peak.max(bytes);
                refused |= bytes > limit;
                bytes <= limit
            },
            &mut st,
        );
        (r, peak, st.parts, refused)
    };
    let (one, peak, parts, _) = run(1, usize::MAX);
    assert_eq!(parts, k as usize);
    let one = one.expect("no limit");
    assert!(one.is_err());
    // A budget for one part at a time, not for eight.
    // (Whether the parts run at the same time depends on the scheduling:
    // the result must not.)
    let (eight, _, _, refused) = run(8, peak + peak / 2);
    eprintln!("8 threads over the budget together: {refused}");
    assert_eq!(eight.expect("one part at a time fits"), one);
    // A budget for none.
    assert!(run(8, peak / 2).0.is_err());
}

#[test]
fn persistence_assumptions_need_no_tableau() {
    use tlx_live::check_lasso_within;
    // `<>[][A_i]_v` assumptions of a specification (one per process) under
    // a property `[]<>p`: the negation is a conjunction of 21 `<>[]`
    // obligations, 2^21 tableau states as one formula. On a ring where
    // A_i (action atom i) is every step but the one out of state i, and
    // stuttering: a behavior with only A_i steps eventually, for every i,
    // ends stuttering; p holds only at state 0.
    let n = 1000u32;
    let k = 20u32;
    let states: Vec<u64> = (0..n).map(|s| u64::from(s == 0)).collect();
    let edges: Vec<(u32, u32, u64)> = (0..n).map(|s| (s, (s + 1) % n, ((1u64 << k) - 1) & !(1 << s.min(63)))).collect();
    let mut g = graph(&[0], &states, &edges, true);
    // Stuttering steps are A_i steps ([A_i]_v).
    for (s, es) in g.edges.iter_mut().enumerate() {
        es.last_mut().expect("stuttering step").1 = Bits::from_u64((1 << k) - 1);
        assert_eq!(es.last().unwrap().0, s as u32);
    }
    let assumptions = and((0..k).map(|i| eventually(always(act(i)))).collect());
    let prop = or(vec![not(assumptions), always(eventually(st(0)))]);
    let mut peak = 0;
    let r = check_lasso_within(&g, &prop, &[], &mut |nodes, _, _| {
        peak = peak.max(nodes);
        true
    });
    // Stuttering at state 1 forever satisfies every assumption, not p.
    let l = r.expect("small").expect_err("stuttering away from 0");
    assert!(lasso_error(&g, &prop, &[], &l).is_none());
    assert!(peak <= n as usize, "{peak} product nodes for {n} states");
}
