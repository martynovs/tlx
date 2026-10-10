//! Framing for checkpoint files (manifest and data files, docs/scale.md
//! §9.2): a header identifying the format, the file kind and the tlx build,
//! then a sequence of checksummed chunks and an end marker.
//!
//! ```text
//! header  magic        8 bytes  "\x89TLXSER\n"
//!         version      u32 LE   FORMAT_VERSION
//!         kind         u32 LE   caller-defined file kind (e.g. a fourcc)
//!         build id     u32 LE length + UTF-8 bytes (`build_id()`)
//!         checksum     u64 LE   of all header bytes above
//! chunk   length       u64 LE   payload length (< END)
//!         checksum     u64 LE   of (chunk index, payload)
//!         payload
//! end     END          u64 LE   u64::MAX
//!         checksum     u64 LE   of (number of chunks)
//! ```
//!
//! All integers are little-endian. Checksums are 64-bit non-cryptographic
//! hashes; they detect torn writes and bit rot, the chunk index in a chunk's
//! checksum detects reordered or duplicated chunks. A file without its end
//! marker is reported as truncated (an append-only data file whose valid
//! length is recorded elsewhere can still be read chunk by chunk up to that
//! length).
//!
//! Recover is only supported with the same tlx build (the compiled tier's
//! state encoding depends on hash functions that are not stable across Rust
//! versions, and the formats are not frozen): readers refuse a file whose
//! build id differs from the running one.

use std::fmt;
use std::io::{self, Read, Write};
use std::sync::OnceLock;

use crate::hash::{fmix64, hash_bytes};

pub const MAGIC: [u8; 8] = *b"\x89TLXSER\n";
/// Version of the framing and of the payload encodings (`ser`, `tlx_layout::ser`).
pub const FORMAT_VERSION: u32 = 1;
const END: u64 = u64::MAX;
/// Longest accepted build id (guards allocation on corrupt headers).
const MAX_BUILD_ID: usize = 4096;

/// Checksum of `bytes` mixed with `seed`.
pub fn checksum(seed: u64, bytes: &[u8]) -> u64 {
    fmix64(hash_bytes(bytes) ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

/// Identity of this tlx build: package version, target, and a hash of the
/// running executable's contents (any code change gives a different id, a
/// rebuild of the same sources normally the same one). Computed once.
pub fn build_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let exe = std::env::current_exe()
            .and_then(std::fs::read)
            .map(|b| format!("{:016x}", checksum(0, &b)))
            .unwrap_or_else(|_| "unknown".to_string());
        format!("tlx {} {}-{} exe:{exe}", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH, std::env::consts::OS)
    })
}

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    /// Not a tlx serialization file.
    BadMagic,
    UnsupportedVersion {
        found: u32,
        expected: u32,
    },
    WrongKind {
        found: u32,
        expected: u32,
    },
    /// Written by a different tlx build.
    BuildMismatch {
        found: String,
        expected: String,
    },
    /// Checksum mismatch in the header or a chunk.
    Checksum {
        what: String,
    },
    /// The data ends before the end marker.
    Truncated {
        what: String,
    },
    Corrupt(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "I/O error: {e}"),
            FrameError::BadMagic => write!(f, "not a tlx checkpoint file (bad magic number)"),
            FrameError::UnsupportedVersion { found, expected } => {
                write!(f, "unsupported checkpoint format version {found} (this tlx reads version {expected})")
            }
            FrameError::WrongKind { found, expected } => {
                write!(f, "unexpected checkpoint file kind {found:#010x} (expected {expected:#010x})")
            }
            FrameError::BuildMismatch { found, expected } => write!(
                f,
                "checkpoint was written by a different tlx build ({found}); this is {expected}; \
                 recover requires the same tlx binary"
            ),
            FrameError::Checksum { what } => write!(f, "checksum mismatch in {what} (corrupt checkpoint file)"),
            FrameError::Truncated { what } => write!(f, "checkpoint file is truncated ({what})"),
            FrameError::Corrupt(m) => write!(f, "corrupt checkpoint file: {m}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> FrameError {
        FrameError::Io(e)
    }
}

pub type FrameResult<T> = Result<T, FrameError>;

/// Writes a framed file: header on creation, then `chunk`s, then `finish`.
pub struct FrameWriter<W: Write> {
    w: W,
    chunks: u64,
}

impl<W: Write> FrameWriter<W> {
    /// Write the header with this build's id.
    pub fn new(w: W, kind: u32) -> FrameResult<FrameWriter<W>> {
        FrameWriter::with_build_id(w, kind, build_id())
    }

    pub fn with_build_id(mut w: W, kind: u32, build: &str) -> FrameResult<FrameWriter<W>> {
        assert!(build.len() <= MAX_BUILD_ID, "build id too long");
        let mut h = Vec::with_capacity(32 + build.len());
        h.extend_from_slice(&MAGIC);
        h.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        h.extend_from_slice(&kind.to_le_bytes());
        h.extend_from_slice(&(build.len() as u32).to_le_bytes());
        h.extend_from_slice(build.as_bytes());
        let c = checksum(0, &h);
        h.extend_from_slice(&c.to_le_bytes());
        w.write_all(&h)?;
        Ok(FrameWriter { w, chunks: 0 })
    }

    pub fn chunk(&mut self, payload: &[u8]) -> FrameResult<()> {
        let mut h = [0u8; 16];
        h[..8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        h[8..].copy_from_slice(&checksum(self.chunks, payload).to_le_bytes());
        self.w.write_all(&h)?;
        self.w.write_all(payload)?;
        self.chunks += 1;
        Ok(())
    }

    /// Number of chunks written so far.
    pub fn chunks(&self) -> u64 {
        self.chunks
    }

    /// Write the end marker and return the underlying writer (not flushed
    /// or synced: durability is the caller's commit protocol).
    pub fn finish(mut self) -> FrameResult<W> {
        let mut h = [0u8; 16];
        h[..8].copy_from_slice(&END.to_le_bytes());
        h[8..].copy_from_slice(&checksum(END, &self.chunks.to_le_bytes()).to_le_bytes());
        self.w.write_all(&h)?;
        Ok(self.w)
    }
}

/// Reads a framed file, verifying the header and every chunk.
pub struct FrameReader<R: Read> {
    r: R,
    chunks: u64,
    done: bool,
    build: String,
}

impl<R: Read> FrameReader<R> {
    /// Read and check the header: magic, format version, `kind` and this
    /// build's id.
    pub fn open(r: R, kind: u32) -> FrameResult<FrameReader<R>> {
        FrameReader::open_with_build_id(r, kind, Some(build_id()))
    }

    /// As `open`, with an explicit expected build id (`None` accepts any
    /// build, e.g. to show what wrote a file).
    pub fn open_with_build_id(mut r: R, kind: u32, expected: Option<&str>) -> FrameResult<FrameReader<R>> {
        let mut h = vec![0u8; 20];
        read_exact(&mut r, &mut h, "header")?;
        if h[..8] != MAGIC {
            return Err(FrameError::BadMagic);
        }
        let le32 = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
        let version = le32(&h[8..12]);
        let found_kind = le32(&h[12..16]);
        let n = le32(&h[16..20]) as usize;
        if n > MAX_BUILD_ID {
            return Err(FrameError::Corrupt(format!("build id length {n}")));
        }
        h.resize(20 + n + 8, 0);
        read_exact(&mut r, &mut h[20..], "header")?;
        let stored = u64::from_le_bytes(h[20 + n..].try_into().unwrap());
        if checksum(0, &h[..20 + n]) != stored {
            return Err(FrameError::Checksum { what: "the header".into() });
        }
        // Version first: a future version may change the rest of the header.
        if version != FORMAT_VERSION {
            return Err(FrameError::UnsupportedVersion { found: version, expected: FORMAT_VERSION });
        }
        let build = String::from_utf8(h[20..20 + n].to_vec()).map_err(|_| FrameError::Corrupt("build id is not UTF-8".into()))?;
        if let Some(exp) = expected {
            if build != exp {
                return Err(FrameError::BuildMismatch { found: build, expected: exp.to_string() });
            }
        }
        if found_kind != kind {
            return Err(FrameError::WrongKind { found: found_kind, expected: kind });
        }
        Ok(FrameReader { r, chunks: 0, done: false, build })
    }

    /// The build id stored in the header.
    pub fn build_id(&self) -> &str {
        &self.build
    }

    /// The next chunk's payload, `None` after the end marker.
    pub fn chunk(&mut self) -> FrameResult<Option<Vec<u8>>> {
        if self.done {
            return Ok(None);
        }
        let what = || format!("chunk {}", self.chunks);
        let mut h = [0u8; 16];
        read_exact(&mut self.r, &mut h, &what())?;
        let len = u64::from_le_bytes(h[..8].try_into().unwrap());
        let sum = u64::from_le_bytes(h[8..].try_into().unwrap());
        if len == END {
            if checksum(END, &self.chunks.to_le_bytes()) != sum {
                return Err(FrameError::Checksum { what: "the end marker".into() });
            }
            self.done = true;
            return Ok(None);
        }
        // Read incrementally: a corrupt length must not allocate up front.
        let mut payload = Vec::new();
        let got = (&mut self.r).take(len).read_to_end(&mut payload)?;
        if (got as u64) < len {
            return Err(FrameError::Truncated { what: what() });
        }
        if checksum(self.chunks, &payload) != sum {
            return Err(FrameError::Checksum { what: what() });
        }
        self.chunks += 1;
        Ok(Some(payload))
    }

    /// All remaining chunks (up to and including the end marker).
    pub fn chunks(&mut self) -> FrameResult<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        while let Some(c) = self.chunk()? {
            out.push(c);
        }
        Ok(out)
    }

    pub fn into_inner(self) -> R {
        self.r
    }
}

fn read_exact<R: Read>(r: &mut R, buf: &mut [u8], what: &str) -> FrameResult<()> {
    r.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => FrameError::Truncated { what: what.to_string() },
        _ => FrameError::Io(e),
    })
}
