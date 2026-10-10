//! Fingerprints of records, record-like functions and sets of strings
//! depend on the string intern order: fields and elements are stored and
//! hashed in `Sym` order (= first-intern order, TLC's string order), not in
//! text order. Within one process that order never changes, so equal values
//! always get equal fingerprints. Across processes (checkpoint / recover)
//! the reader must restore the writer's intern table before anything else
//! interns; these tests pin both halves: a fresh process that restores the
//! table reproduces the writer's fingerprints and values, and one that
//! interns the same texts in another order gets other fingerprints and
//! cannot decode the writer's records.

use std::path::PathBuf;
use std::process::Command;

use tlx_value::fingerprint::fingerprint;
use tlx_value::ser::{decode_value, encode_value, read_intern_table, write_intern_table, Reader, Writer};
use tlx_value::{intern, Sym, Value};

const FILE_ENV: &str = "SYM_ORDER_TEST_FILE";
const MODE_ENV: &str = "SYM_ORDER_TEST_MODE";

// Names no other test interns (each test binary is its own process, but the
// tests in it share one table): interned "b" before "a" by `values`.
const B: &str = "zz_sym_order_b";
const A: &str = "zz_sym_order_a";

/// A record, the equal function built from pairs, and a set of the names.
fn values(a: Sym, b: Sym) -> Vec<Value> {
    let rec = Value::record(vec![(a, Value::Int(1)), (b, Value::Int(2))]);
    let fcn = Value::fcn_from_pairs(vec![(Value::Str(b), Value::Int(2)), (Value::Str(a), Value::Int(1))]).unwrap();
    let set = Value::set_from(vec![Value::Str(a), Value::Str(b)]).unwrap();
    vec![rec, fcn, set]
}

/// Within a process: any input order gives the same stored (intern) order
/// and the same fingerprint, for every representation of the value.
#[test]
fn equal_values_have_equal_fingerprints_in_any_input_order() {
    let b = Sym::new("zz_sym_order_local_b");
    let a = Sym::new("zz_sym_order_local_a");
    assert!(b < a, "Sym order is intern order");
    let r1 = Value::record(vec![(a, Value::Int(1)), (b, Value::Int(2))]);
    let r2 = Value::record(vec![(b, Value::Int(2)), (a, Value::Int(1))]);
    let Value::Record(fs) = &r1 else { panic!("{r1:?}") };
    assert_eq!(fs.iter().map(|f| f.0).collect::<Vec<_>>(), vec![b, a], "fields are stored in intern order, not text order");
    let f = Value::fcn_from_pairs(vec![(Value::Str(a), Value::Int(1)), (Value::Str(b), Value::Int(2))]).unwrap();
    let as_fcn = r1.clone().into_fcn_rcd();
    for v in [&r2, &f, &as_fcn] {
        assert_eq!(&r1, v);
        assert_eq!(fingerprint(&r1).unwrap(), fingerprint(v).unwrap(), "{v:?}");
    }
    let s1 = Value::set_from(vec![Value::Str(a), Value::Str(b)]).unwrap();
    let s2 = Value::set_from(vec![Value::Str(b), Value::Str(a)]).unwrap();
    assert_eq!(fingerprint(&s1).unwrap(), fingerprint(&s2).unwrap());
}

/// The writer: interns `B` before `A`, writes its intern table, the encoded
/// values and their fingerprints, and runs this binary as a fresh reader
/// process twice (restoring the table first, and interning in text order).
#[test]
fn fresh_process_reproduces_fingerprints_only_after_restoring_the_order() {
    let b = Sym::new(B);
    let a = Sym::new(A);
    let vs = values(a, b);
    let mut w = Writer::new();
    write_intern_table(&mut w);
    w.usize(vs.len());
    for v in &vs {
        w.bytes(&encode_value(v).unwrap());
        w.uvar(fingerprint(v).unwrap());
    }
    let dir = tlx_testkit::TempDir::new("sym-order");
    let path = dir.path().join("values.bin");
    std::fs::write(&path, w.as_bytes()).unwrap();
    for mode in ["restore", "text-order"] {
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_reader", "--ignored", "--nocapture", "--test-threads=1"])
            .env(FILE_ENV, &path)
            .env(MODE_ENV, mode)
            .output()
            .unwrap();
        let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "reader ({mode}) failed:\n{text}");
        assert!(text.contains("1 passed"), "reader ({mode}) did not run:\n{text}");
    }
}

/// The reader process (run by the test above).
#[test]
#[ignore]
fn child_reader() {
    let Some(path) = std::env::var_os(FILE_ENV).map(PathBuf::from) else { return };
    let mode = std::env::var(MODE_ENV).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let mut r = Reader::new(&bytes);
    let texts = read_intern_table(&mut r).unwrap();
    let n = r.usize().unwrap();
    let written: Vec<(Vec<u8>, u64)> = (0..n).map(|_| (r.bytes().unwrap().to_vec(), r.uvar().unwrap())).collect();
    r.finish().unwrap();
    assert_eq!(intern::len(), 0, "something interned before the reader");

    if mode == "restore" {
        // Recover: the writer's order first, then values built afresh (in
        // any input order) and decoded ones match the writer's fingerprints.
        intern::restore(&texts).unwrap();
        let rebuilt = values(Sym::new(A), Sym::new(B));
        for ((stream, fp), v) in written.iter().zip(&rebuilt) {
            assert_eq!(fingerprint(v).unwrap(), *fp, "rebuilt {v:?}");
            let d = decode_value(stream).unwrap();
            assert_eq!(&d, v);
            assert_eq!(fingerprint(&d).unwrap(), *fp, "decoded {d:?}");
        }
        return;
    }

    // Without the restore, interning in text order: the same values get
    // other fingerprints (a writer's fingerprint set would then miss
    // duplicates), and decoding the writer's record is refused.
    let a = Sym::new(A);
    let b = Sym::new(B);
    assert!(a < b);
    let rebuilt = values(a, b);
    for ((_, fp), v) in written.iter().zip(&rebuilt) {
        assert_ne!(fingerprint(v).unwrap(), *fp, "{v:?}: the fingerprint does not depend on the intern order?");
    }
    let e = decode_value(&written[0].0).unwrap_err().to_string();
    assert!(e.contains("intern order"), "{e}");
}
