//! Cross-process recover of the string intern order and the `Dyn` interner:
//! this process builds states with `Dyn` slots and strings interned in a
//! non-alphabetical order, writes a framed snapshot, and re-runs this test
//! binary as a fresh process that restores the snapshot and checks that
//! handles, intern order and decoded states are unchanged.

use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

use tlx_base::frame::{FrameReader, FrameWriter};
use tlx_layout::shape::Shape;
use tlx_layout::{dynval, ser, Layout};
use tlx_value::ser::{read_intern_table, write_intern_table, Reader, Writer};
use tlx_value::{intern, Sym, Value};

const KIND: u32 = u32::from_le_bytes(*b"TSRT");
const FILE_ENV: &str = "TLX_SER_RECOVER_FILE";
const MODE_ENV: &str = "TLX_SER_RECOVER_MODE";

fn states() -> Vec<Vec<Value>> {
    // `b` before `a`: Sym order differs from text order.
    let b = Sym::new("zz_ser_rec_b");
    let a = Sym::new("zz_ser_rec_a");
    let set = Value::set_from(vec![Value::Str(a), Value::Str(b)]).unwrap();
    (0..50)
        .map(|i| {
            vec![
                // Dyn slot: a value of varying kind.
                if i % 2 == 0 {
                    Value::tuple((0..i).map(Value::Int).collect())
                } else {
                    Value::record(vec![(a, Value::Int(i)), (b, set.clone())])
                },
                Value::record(vec![(a, Value::Int(i % 4)), (b, Value::Bool(i % 3 == 0))]),
                set.clone(),
            ]
        })
        .collect()
}

#[test]
fn recover_in_fresh_process() {
    let states = states();
    let shapes: Vec<Shape> =
        vec![Shape::Dyn, states.iter().map(|s| Shape::of(&s[1])).fold(Shape::Never, |x, y| x.join(&y)), Shape::of(&states[0][2])];
    let layout = Layout::new(shapes);
    let mut words = vec![0u64; layout.words * states.len()];
    for (s, out) in states.iter().zip(words.chunks_mut(layout.words)) {
        layout.encode_state(s, out).unwrap();
    }

    let mut f = FrameWriter::new(Vec::new(), KIND).unwrap();
    let mut w = Writer::new();
    write_intern_table(&mut w);
    f.chunk(w.as_bytes()).unwrap();
    w.clear();
    ser::write_dyn_table(&mut w, &dynval::snapshot()).unwrap();
    f.chunk(w.as_bytes()).unwrap();
    w.clear();
    ser::write_layout(&mut w, &layout);
    w.usize(states.len());
    for x in &words {
        w.uvar(*x);
    }
    for s in &states {
        for v in s {
            w.value(v).unwrap();
        }
    }
    f.chunk(w.as_bytes()).unwrap();
    let file = f.finish().unwrap();

    let dir = tlx_testkit::TempDir::new("ser-recover");
    let path = dir.path().join("snapshot.bin");
    std::fs::write(&path, &file).unwrap();
    let run = |mode: &str| {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_restore", "--ignored", "--nocapture", "--test-threads=1"])
            .env(FILE_ENV, &path)
            .env(MODE_ENV, mode)
            .output()
            .unwrap()
    };
    let ok = run("restore");
    let late = run("intern-first");
    let show = |o: &std::process::Output| format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(ok.status.success(), "child failed:\n{}", show(&ok));
    assert!(show(&ok).contains("1 passed"), "child test did not run:\n{}", show(&ok));
    assert!(late.status.success(), "child (intern-first) failed:\n{}", show(&late));
    assert!(show(&late).contains("1 passed"), "child test did not run:\n{}", show(&late));
}

/// The recovering process (run by `recover_in_fresh_process`).
#[test]
#[ignore]
fn child_restore() {
    let Some(path) = std::env::var_os(FILE_ENV).map(PathBuf::from) else { return };
    let mode = std::env::var(MODE_ENV).unwrap();
    let file = std::fs::read(path).unwrap();
    let mut f = FrameReader::open(Cursor::new(&file), KIND).unwrap();
    let chunks = f.chunks().unwrap();
    assert_eq!(chunks.len(), 3);

    if mode == "intern-first" {
        // Interning before the restore breaks the recorded order: refused.
        Sym::new("zz_ser_rec_a");
        let mut r = Reader::new(&chunks[0]);
        let texts = read_intern_table(&mut r).unwrap();
        let e = intern::restore(&texts).unwrap_err();
        assert!(e.contains("intern order"), "{e}");
        return;
    }

    // 1. The string intern table, before anything else interns.
    assert_eq!(intern::len(), 0, "something interned before the restore");
    let mut r = Reader::new(&chunks[0]);
    let texts = read_intern_table(&mut r).unwrap();
    r.finish().unwrap();
    intern::restore(&texts).unwrap();
    assert!(Sym::new("zz_ser_rec_b") < Sym::new("zz_ser_rec_a"));

    // 2. The Dyn interner.
    let mut r = Reader::new(&chunks[1]);
    let snap = ser::read_dyn_table(&mut r).unwrap();
    r.finish().unwrap();
    dynval::restore(&snap).unwrap();

    // 3. Layout and encoded states: decoding the old words (Dyn handles
    // included) gives the old states, and re-encoding gives the old words.
    let mut r = Reader::new(&chunks[2]);
    let layout = ser::read_layout(&mut r).unwrap();
    let n = r.usize().unwrap();
    let words: Vec<u64> = (0..n * layout.words).map(|_| r.uvar().unwrap()).collect();
    let expected: Vec<Vec<Value>> = (0..n).map(|_| (0..layout.vars.len()).map(|_| r.value().unwrap()).collect()).collect();
    r.finish().unwrap();
    assert_eq!(expected, states());
    let mut buf = vec![0u64; layout.words];
    for (s, ws) in expected.iter().zip(words.chunks(layout.words)) {
        assert_eq!(&layout.decode_state(ws), s);
        layout.encode_state(s, &mut buf).unwrap();
        assert_eq!(&buf[..], ws);
    }
    // Values decoded from the stream are canonical in the restored order.
    let set = &expected[0][2];
    let Value::Set(es) = set else { panic!("{set}") };
    assert_eq!(Value::set_from(es.to_vec()).unwrap().to_string(), set.to_string());
}
