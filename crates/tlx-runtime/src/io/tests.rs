use super::*;

fn spill_in(dir: &Path) -> Spill {
    Spill::new(SpillConfig { dir: Some(dir.to_path_buf()), ..Default::default() })
}

fn data(n: usize, seed: u64) -> Vec<u8> {
    (0..n).map(|i| ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(seed) >> 29) as u8).collect()
}

#[test]
fn store_round_trip() {
    let scratch = tlx_testkit::TempDir::new("io-store");
    let dir = scratch.child_path("spill");
    let spill = spill_in(&dir);
    let store = spill.store("blocks").unwrap();
    let blocks: Vec<Vec<u8>> = [0, 1, 4095, 4096, 4097, 100_000].iter().enumerate().map(|(k, &n)| data(n, k as u64)).collect();
    let refs: Vec<BlockRef> = blocks.iter().map(|b| store.append(b).unwrap()).collect();
    for (b, r) in blocks.iter().zip(&refs) {
        assert_eq!(&store.read(r).unwrap(), b);
        // Ranges across page boundaries.
        for (at, len) in [(0, 1), (4090, 12), (5000, 3000)] {
            if at + len <= b.len() {
                let mut out = vec![0; len];
                store.read_range(r, at, &mut out).unwrap();
                assert_eq!(&out, &b[at..at + len]);
            }
        }
    }
    let c = spill.counters();
    assert_eq!(c.used, blocks.iter().map(|b| b.len() as u64).sum::<u64>());
    assert_eq!(c.blocks, blocks.len() as u64);
    store.delete(refs[5].clone());
    assert_eq!(spill.counters().used, c.used - 100_000);
    assert!(spill.error().is_none());
    // The directory did not exist: tlx creates it and removes it with
    // the last store, while the RAII parent stays alive.
    assert!(dir.join("LOCK").exists());
    drop(store);
    assert!(!dir.exists());
    assert!(scratch.is_dir(), "tlx removes the spill child before its RAII parent is dropped");
}

#[test]
fn panic_removes_owned_spill_resources() {
    let scratch = tlx_testkit::TempDir::new("io-panic");
    let parent = scratch.path().to_path_buf();
    let result = std::panic::catch_unwind(move || {
        let dir = scratch.child_path("spill");
        let spill = spill_in(&dir);
        let store = spill.store("blocks").unwrap();
        store.append(b"written before panic").unwrap();
        assert!(dir.join("LOCK").is_file());
        panic!("intentional panic after creating spill resources");
    });
    assert!(result.is_err());
    assert!(!parent.exists(), "the owned parent and spill child survived unwinding");
}

#[test]
fn checksum_detects_corruption() {
    let scratch = tlx_testkit::TempDir::new("io-corrupt");
    let dir = scratch.child_path("spill");
    let spill = spill_in(&dir);
    let store = spill.store("blocks").unwrap();
    let b = data(10_000, 7);
    let r = store.append(&b).unwrap();
    // Flip a byte of the second page behind the store's back.
    let f = OpenOptions::new().write(true).open(store.path()).unwrap();
    f.write_all_at(&[b[5000] ^ 0x40], r.off + 5000).unwrap();
    let e = store.read(&r).unwrap_err();
    assert!(e.0.contains("checksum mismatch at offset 4096"), "{e}");
    // The first page is still fine on its own.
    let mut out = [0u8; 16];
    store.read_range(&r, 100, &mut out).unwrap();
    assert_eq!(spill.error(), Some(e));
}

use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;

#[test]
fn maxdisk_quota() {
    let scratch = tlx_testkit::TempDir::new("io-quota");
    let dir = scratch.child_path("spill");
    let spill = Spill::new(SpillConfig { dir: Some(dir.clone()), maxdisk: Some(64 << 10), ..Default::default() });
    let store = spill.store("blocks").unwrap();
    for k in 0..4 {
        store.append(&data(16 << 10, k)).unwrap();
    }
    let e = store.append(&[0u8; 1]).unwrap_err();
    assert!(e.0.contains("-maxdisk 64 KiB") && e.0.contains(&dir.display().to_string()), "{e}");
    assert!(spill.failed());
    // Deleting makes room again.
    let r = store.append(&[]).unwrap();
    store.delete(r);
}

#[test]
fn injected_write_error_keeps_data() {
    let scratch = tlx_testkit::TempDir::new("io-inject");
    let dir = scratch.child_path("spill");
    let spill = Spill::new(SpillConfig { dir: Some(dir.clone()), fail_after: Some(3 * 1024), ..Default::default() });
    let mut v: SpillVec<u64> = SpillVec::new(1024, "log");
    for i in 0..1000u64 {
        v.push(i * 3);
    }
    // Blocks of 128 values (1 KiB): the first three are written, the
    // others fail and stay in memory.
    v.spill(&spill, false).unwrap_or(());
    let e = v.settle().unwrap_err();
    assert!(e.0.contains("injected write error"), "{e}");
    assert_eq!(spill.error(), Some(e));
    assert_eq!(v.spilled_blocks(), 3);
    for i in 0..1000 {
        assert_eq!(v.get(i).unwrap(), i as u64 * 3);
    }
    assert_eq!(v.iter(Some(&spill)).collect::<Vec<_>>(), (0..1000).map(|i| i * 3).collect::<Vec<_>>());
}

#[test]
fn spillvec_round_trip() {
    let scratch = tlx_testkit::TempDir::new("io-vec");
    let dir = scratch.child_path("spill");
    let spill = spill_in(&dir);
    let mut v: SpillVec<[u64; 2]> = SpillVec::new(4096, "trace");
    assert_eq!(v.block_len(), 256);
    let n = 10_000u64;
    for i in 0..n {
        assert_eq!(v.push([i, !i]), i as usize);
        if i % 1000 == 999 {
            v.spill(&spill, false).unwrap();
        }
    }
    v.settle().unwrap();
    assert!(v.spilled_blocks() >= 35, "{}", v.spilled_blocks());
    assert!(v.bytes() <= 2 * 4096, "{}", v.bytes());
    for i in (0..n).step_by(7) {
        assert_eq!(v.get(i as usize).unwrap(), [i, !i]);
    }
    // Everything, the partial last block too; then appending again reads
    // that block back.
    v.spill(&spill, true).unwrap();
    v.settle().unwrap();
    assert_eq!(v.spilled_blocks(), n.div_ceil(256) as usize);
    v.unseal().unwrap();
    v.push([n, !n]);
    v.set(5, [55, 55]).unwrap();
    let all: Vec<[u64; 2]> = v.iter(Some(&spill)).collect();
    assert_eq!(all.len(), n as usize + 1);
    assert_eq!(all[5], [55, 55]);
    assert_eq!(all[n as usize], [n, !n]);
    assert!(spill.counters().read > 0);
    let before = spill.counters().used;
    let mut it = v.drain(Some(&spill));
    let drained: Vec<[u64; 2]> = it.by_ref().collect();
    assert!(it.error().is_none());
    assert_eq!(drained, all);
    // (Draining gives the disk space back.)
    assert!(spill.counters().used < before);
    drop(it);
    assert!(!dir.exists(), "the spill directory is removed with its last store");
    assert!(spill.error().is_none());
}

#[test]
fn concurrent_appends() {
    let scratch = tlx_testkit::TempDir::new("io-concurrent");
    let dir = scratch.child_path("spill");
    let spill = spill_in(&dir);
    let store = spill.store("blocks").unwrap();
    let refs: Vec<Vec<(u64, BlockRef)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..8u64)
            .map(|t| {
                let store = &store;
                sc.spawn(move || {
                    (0..50u64).map(|k| (t * 100 + k, store.append(&data(3000 + k as usize * 37, t * 100 + k)).unwrap())).collect()
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (seed, r) in refs.into_iter().flatten() {
        assert_eq!(store.read(&r).unwrap(), data(r.len, seed));
    }
}

#[test]
fn wait_for_writes_finishes_and_releases_stores() {
    let scratch = tlx_testkit::TempDir::new("io-wait-writes");
    let dir = scratch.child_path("spill");
    let spill = spill_in(&dir);
    spill.wait_for_writes();
    assert!(spill.dir().is_none(), "waiting without writes creates nothing");
    let store = spill.store("blocks").unwrap();
    let pending: Vec<_> = (0..8).map(|_| spill.write_async(&store, Box::new(vec![42u8; 4096]))).collect();
    drop(store);
    spill.wait_for_writes();
    assert!(pending.iter().all(|p| matches!(p.try_done(), Some(Ok(_)))));
    assert_eq!(spill.counters().written, 8 * 4096);
    assert!(spill.error().is_none());
    assert!(spill.dir().is_none(), "the writer released its last store reference");
    assert!(!dir.exists());
}

#[test]
fn async_writes_and_prefetch() {
    let scratch = tlx_testkit::TempDir::new("io-async");
    let dir = scratch.child_path("spill");
    let spill = spill_in(&dir);
    let store = spill.store("blocks").unwrap();
    let pend: Vec<_> = (0..40u64).map(|k| spill.write_async(&store, Box::new((0..5000u64).map(|i| i ^ k).collect::<Vec<u64>>()))).collect();
    let refs: Vec<BlockRef> = pend.iter().map(|p| p.wait().unwrap()).collect();
    assert!(pend.iter().all(|p| p.with_data(|_| ()).is_none()), "written blocks free their data");
    let pre: Vec<_> = refs.iter().map(|r| spill.prefetch(&store, r)).collect();
    for (k, p) in pre.iter().enumerate() {
        let b = p.wait().unwrap();
        let mut v = vec![0u64; 5000];
        as_bytes_mut(&mut v).copy_from_slice(&b);
        assert!(v.iter().enumerate().all(|(i, &x)| x == i as u64 ^ k as u64));
    }
}

#[test]
fn default_directory_and_lock() {
    let scratch = tlx_testkit::TempDir::new("io-base");
    let base = scratch.path().to_path_buf();
    let spill = Spill::new(SpillConfig { base: Some(base.clone()), ..Default::default() });
    assert!(spill.dir().is_none(), "no directory before the first store");
    let s1 = spill.store("a").unwrap();
    let d = spill.dir().unwrap();
    assert_eq!(d.parent().unwrap(), base.join("states"));
    // Another run on the same spec gets its own directory.
    let other = Spill::new(SpillConfig { base: Some(base.clone()), ..Default::default() });
    let s2 = other.store("a").unwrap();
    assert_ne!(other.dir().unwrap(), d);
    // A directory in use cannot be shared.
    let third = Spill::new(SpillConfig { dir: Some(d.clone()), ..Default::default() });
    let e = third.store("a").err().unwrap();
    assert!(e.0.contains("in use by another process"), "{e}");
    drop((s1, s2));
    assert!(!base.join("states").exists(), "the states directory tlx created is removed when empty");
}

#[test]
fn sizes() {
    assert_eq!(parse_size("512"), Some(512 << 20));
    assert_eq!(parse_size("8G"), Some(8 << 30));
    assert_eq!(parse_size("64k"), Some(64 << 10));
    for bad in ["0", "x", "1.5", "-1", ""] {
        assert_eq!(parse_size(bad), None, "{bad}");
    }
    assert_eq!(SpillMode::parse("Always"), Some(SpillMode::Always));
    assert_eq!(SpillMode::parse("maybe"), None);
    assert_eq!(disk_reserve(10 << 30), (10 << 30) / 50);
    assert_eq!(disk_reserve(1 << 40), 1 << 30);
    assert!(disk_space(&std::env::temp_dir()).is_some());
}

#[test]
fn write_creating_parents_and_append() {
    let scratch = tlx_testkit::TempDir::new("io-files");
    let dir = scratch.child_path("spill");
    let f = dir.join("a/b/out.txt");
    write_creating_parents(&f, b"one").unwrap();
    write_creating_parents(&f, b"two").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"two");
    append_to(&f, b"\nthree").unwrap();
    append_to(&dir.join("new.csv"), b"x\n").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), b"two\nthree");
    assert_eq!(std::fs::read(dir.join("new.csv")).unwrap(), b"x\n");
    // A parent that cannot be created: the write's error.
    std::fs::write(dir.join("file"), b"").unwrap();
    assert!(write_creating_parents(&dir.join("file/x"), b"").is_err());
}
