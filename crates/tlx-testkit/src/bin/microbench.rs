//! Micro-benchmarks of hot paths (docs/benchmarking.md), without
//! criterion: value fingerprints, TLC value comparison, the layout codec
//! (encode / decode of a state's values into words) and the fingerprint of
//! encoded words.
//!
//! `cargo run --profile test-release -p tlx-testkit --features models --bin microbench [filter] [-- reps]`
//!
//! Each benchmark runs `reps` (default 5) timed batches and prints the
//! fastest batch per operation: on a loaded machine the minimum is the
//! least noisy number (see the tlx-perf skill). The workload is fixed
//! (seeded), so numbers are comparable between builds on one machine.

use std::hint::black_box;
use std::time::Instant;

use tlx_layout::{LayoutNode, Shape};
use tlx_testkit::Rng;
use tlx_value::{Sym, Value};

/// A PaxosCommit-like state: per-resource-manager states (a function on
/// model values), a ballot per acceptor, and a set of message records.
fn state(rng: &mut Rng, rms: usize, msgs: usize) -> Vec<Value> {
    let rm = |i: usize| Value::model_value(&format!("r{i}"));
    let phase = ["working", "prepared", "committed", "aborted"];
    let rm_state = Value::fcn_from_pairs((0..rms).map(|i| (rm(i), Value::str(phase[rng.below(4) as usize]))).collect()).unwrap();
    let ballots = Value::tuple((0..3).map(|_| Value::Int(rng.below(4) as i64)).collect());
    let ty = Sym::new("type");
    let (bal, val, from) = (Sym::new("bal"), Sym::new("val"), Sym::new("rm"));
    let msgs = Value::set_from(
        (0..msgs)
            .map(|_| match rng.below(2) {
                0 => Value::record(vec![
                    (ty, Value::str("phase2a")),
                    (bal, Value::Int(rng.below(4) as i64)),
                    (from, rm(rng.below(rms as u64) as usize)),
                ]),
                _ => Value::record(vec![
                    (ty, Value::str("phase2b")),
                    (val, Value::str(phase[1 + rng.below(2) as usize])),
                    (from, rm(rng.below(rms as u64) as usize)),
                ]),
            })
            .collect(),
    )
    .unwrap();
    vec![rm_state, ballots, msgs]
}

/// Run `f` over `n` items `reps` times; print the best ns per item.
fn bench(name: &str, filter: &str, reps: usize, n: usize, mut f: impl FnMut()) {
    if !name.contains(filter) {
        return;
    }
    f(); // warm up
    let best = (0..reps)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_nanos() as f64 / n as f64
        })
        .fold(f64::INFINITY, f64::min);
    println!("{name:32} {best:10.1} ns/op");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let filter = args.iter().find(|a| a.parse::<usize>().is_err()).map_or("", String::as_str);
    let reps = args.iter().find_map(|a| a.parse().ok()).unwrap_or(5);
    let mut rng = Rng::new(1);
    let states: Vec<Vec<Value>> = (0..2000).map(|_| state(&mut rng, 5, 12)).collect();
    let n = states.len();

    bench("value fingerprint (state)", filter, reps, n, || {
        for s in &states {
            for v in s {
                black_box(tlx_value::fingerprint::fingerprint(v).unwrap());
            }
        }
    });
    bench("value cmp_tlc (msg sets)", filter, reps, n - 1, || {
        for w in states.windows(2) {
            black_box(tlx_value::order::cmp_tlc(&w[0][2], &w[1][2]).unwrap());
        }
    });
    bench("value eq (state)", filter, reps, n - 1, || {
        for w in states.windows(2) {
            black_box(w[0] == w[1]);
        }
    });

    // One layout for all states: the join of their shapes.
    let shapes: Vec<Shape> = (0..3).map(|i| states.iter().map(|s| Shape::of(&s[i])).reduce(|a, b| a.join(&b)).unwrap()).collect();
    let nodes: Vec<LayoutNode> = shapes.iter().map(LayoutNode::new).collect();
    let widths: Vec<usize> = nodes.iter().map(LayoutNode::words).collect();
    let total: usize = widths.iter().sum();
    let mut words = vec![0u64; total];
    let encode = |s: &[Value], words: &mut [u64]| {
        words.fill(0);
        let mut o = 0;
        for (i, (node, v)) in nodes.iter().zip(s).enumerate() {
            node.encode(v, &mut words[o..o + widths[i]]).expect("value fits its joined shape");
            o += widths[i];
        }
    };
    let encoded: Vec<Vec<u64>> = states
        .iter()
        .map(|s| {
            encode(s, &mut words);
            words.clone()
        })
        .collect();
    println!("(state: {total} words)");
    bench("layout encode (state)", filter, reps, n, || {
        for s in &states {
            encode(s, &mut words);
            black_box(&words);
        }
    });
    bench("layout decode (state)", filter, reps, n, || {
        for w in &encoded {
            let mut o = 0;
            for (i, node) in nodes.iter().enumerate() {
                black_box(node.decode(&w[o..o + widths[i]]));
                o += widths[i];
            }
        }
    });
    bench("fp_words (encoded state)", filter, reps, n, || {
        for w in &encoded {
            black_box(tlx_native::fp_words(w));
        }
    });
}
