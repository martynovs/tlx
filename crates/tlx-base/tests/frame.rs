//! Frozen fixtures for the generic frame format (`tlx_base::frame`).
//!
//! The frame layout, the build identity and the checksums are a persisted
//! contract: a changed header layout, `checksum` or `hash_bytes` must fail
//! here rather than surface as unreadable checkpoints. These tests pin the
//! exact bytes and cover round trip, truncation, corruption, chunk swapping
//! and build/version refusal.

use std::io::Cursor;

use tlx_base::frame::{build_id, checksum, FrameError, FrameReader, FrameWriter};

const KIND: u32 = u32::from_le_bytes(*b"TEST");

/// Build a byte string from its hex (a frozen fixture: see
/// `frame_bytes_are_frozen`).
fn hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd-length hex fixture");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

// `build_id` reads the executable file to hash it; under Miri that means
// emulating `current_exe` + a whole-file read of the (huge) interpreter
// binary, which does not finish in reasonable time and is unrelated to
// the framing code under test.
#[cfg_attr(miri, ignore)]
#[test]
fn frame_round_trip_and_checks() {
    let payloads: Vec<Vec<u8>> = vec![vec![], b"hello".to_vec(), (0..10_000).map(|i| i as u8).collect()];
    let mut f = FrameWriter::new(Vec::new(), KIND).unwrap();
    for p in &payloads {
        f.chunk(p).unwrap();
    }
    let file = f.finish().unwrap();

    let mut rd = FrameReader::open(Cursor::new(&file), KIND).unwrap();
    assert_eq!(rd.build_id(), build_id());
    assert_eq!(rd.chunks().unwrap(), payloads);
    assert!(rd.chunk().unwrap().is_none());

    let err = |b: &[u8], kind: u32| FrameReader::open(Cursor::new(b), kind).and_then(|mut r| r.chunks()).unwrap_err();
    assert!(matches!(err(&file, KIND + 1), FrameError::WrongKind { .. }));
    assert!(matches!(err(b"not a frame at all, really", KIND), FrameError::BadMagic));
    // Every truncation is detected.
    for cut in 0..file.len() {
        let e = err(&file[..cut], KIND);
        assert!(matches!(e, FrameError::Truncated { .. } | FrameError::BadMagic), "cut {cut}: {e}");
    }
    // Every single-byte corruption is detected.
    for i in 0..file.len() {
        let mut b = file.clone();
        b[i] ^= 0x41;
        let _ = err(&b, KIND);
    }
    // Swapped chunks are detected (the chunk index is part of the checksum).
    let mut f = FrameWriter::new(Vec::new(), KIND).unwrap();
    f.chunk(b"aaaa").unwrap();
    f.chunk(b"bbbb").unwrap();
    let mut b = f.finish().unwrap();
    let hdr = b.len() - 2 * 20 - 16;
    let (x, y) = b[hdr..hdr + 40].split_at(20);
    let swapped: Vec<u8> = [y, x].concat();
    b[hdr..hdr + 40].copy_from_slice(&swapped);
    assert!(matches!(err(&b, KIND), FrameError::Checksum { .. }));

    // A different version is refused with a clear message.
    let mut v2 = file.clone();
    v2[8] = 2;
    let n = 20 + u32::from_le_bytes(v2[16..20].try_into().unwrap()) as usize;
    let c = checksum(0, &v2[..n]);
    v2[n..n + 8].copy_from_slice(&c.to_le_bytes());
    let e = err(&v2, KIND);
    assert!(matches!(e, FrameError::UnsupportedVersion { found: 2, .. }), "{e}");
}

// Same reason as `frame_round_trip_and_checks`: `build_id` hashes the
// executable, too slow (and isolation-dependent) under Miri.
#[cfg_attr(miri, ignore)]
#[test]
fn frame_refuses_other_build() {
    const KIND: u32 = 7;
    let mut f = FrameWriter::with_build_id(Vec::new(), KIND, "tlx 0.0.0 other-build").unwrap();
    f.chunk(b"x").unwrap();
    let file = f.finish().unwrap();
    let e = FrameReader::open(Cursor::new(&file), KIND).err().expect("must refuse");
    let msg = e.to_string();
    assert!(matches!(e, FrameError::BuildMismatch { .. }));
    assert!(msg.contains("different tlx build") && msg.contains("other-build"), "{msg}");
    // Inspection without the check still works.
    let mut r = FrameReader::open_with_build_id(Cursor::new(&file), KIND, None).unwrap();
    assert_eq!(r.build_id(), "tlx 0.0.0 other-build");
    assert_eq!(r.chunks().unwrap(), vec![b"x".to_vec()]);
}

/// The frame layout, the build identity and the checksums are a persisted
/// contract: this file's exact bytes (a fixed build id, three chunks
/// including the empty one and a non-word-aligned tail) are a frozen
/// fixture. A changed header layout, `checksum` or `hash_bytes` must fail
/// here rather than surface as unreadable checkpoints.
#[test]
fn frame_bytes_are_frozen() {
    const GOLD: u32 = u32::from_le_bytes(*b"GOLD");
    let mut f = FrameWriter::with_build_id(Vec::new(), GOLD, "tlx 0.0.0 golden").unwrap();
    f.chunk(b"").unwrap();
    f.chunk(b"hello, frame").unwrap();
    f.chunk(&[0, 1, 2, 253, 254, 255]).unwrap();
    let file = f.finish().unwrap();
    let expect = hex(concat!(
        // header: magic, version 1, kind "GOLD", build id, its checksum
        "89544c585345520a01000000474f4c4410000000",
        "746c7820302e302e3020676f6c64656e",
        "c8cd5fbe5117adfb",
        // chunks: length, checksum, payload (empty, "hello, frame", 6 bytes)
        "0000000000000000d44ae93f933c1dee",
        "0c000000000000009e8f019c7eeb6aaf68656c6c6f2c206672616d65",
        "06000000000000003a46bf354eb3cfd9000102fdfeff",
        // end marker and its checksum
        "ffffffffffffffffa3e935062a5dea18",
    ));
    assert_eq!(file, expect);

    // And the frozen file reads back chunk by chunk.
    let mut r = FrameReader::open_with_build_id(Cursor::new(&expect), GOLD, None).unwrap();
    assert_eq!(r.build_id(), "tlx 0.0.0 golden");
    assert_eq!(r.chunks().unwrap(), vec![Vec::new(), b"hello, frame".to_vec(), vec![0, 1, 2, 253, 254, 255]]);
}

/// `checksum` mixes the seed in before the hash: fixed vectors.
#[test]
fn checksum_vectors() {
    assert_eq!(checksum(0, b"hello, frame"), 0x6d14_3263_f9a1_7548);
    assert_eq!(checksum(7, b""), 0x3eee_1437_ae43_09d3);
}
