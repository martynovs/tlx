//! Compile generated C into a shared library, cached by content hash.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Options for the C compiler.
#[derive(Clone, Debug)]
pub struct BuildOptions {
    /// Compiler executable (default: `$CC` or `cc`).
    pub cc: String,
    /// Optimization flags (default `-O2`).
    pub opt: String,
    /// Debug build with sanitizers (default: whether `$TLX_CC_SANITIZE` is
    /// set). The host process must then load the sanitizer runtimes, e.g.
    /// on macOS `DYLD_INSERT_LIBRARIES=$(cc -print-file-name=libclang_rt.asan_osx_dynamic.dylib)`.
    pub debug: bool,
    /// Cache directory (default: `$TLX_CACHE` or `~/.cache/tlx`).
    pub cache_dir: PathBuf,
}

impl Default for BuildOptions {
    fn default() -> BuildOptions {
        let env = tlx_base::knobs::Env::process();
        let cache_dir = env.path("TLX_CACHE").unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
            home.join(".cache").join("tlx")
        });
        BuildOptions { cc: env.string_or_default("CC"), opt: "-O2".into(), debug: env.flag("TLX_CC_SANITIZE"), cache_dir }
    }
}

/// FNV-1a over the inputs that determine the build output.
fn content_hash(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn dylib_ext() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Where the build of `source` with `opts` is cached: (directory, library
/// path, compiler flags).
fn cache_slot<'a>(source: &str, opts: &'a BuildOptions) -> (PathBuf, PathBuf, &'a str) {
    let flags = if opts.debug { "-O1 -g -fsanitize=address,undefined" } else { opts.opt.as_str() };
    let key = content_hash(&[source.as_bytes(), crate::ABI_HEADER.as_bytes(), opts.cc.as_bytes(), flags.as_bytes()]);
    let dir = opts.cache_dir.join(format!("{key:016x}"));
    let lib = dir.join(format!("model.{}", dylib_ext()));
    (dir, lib, flags)
}

/// The cached library of `source` built with `opts`, if there is one.
pub fn cached(source: &str, opts: &BuildOptions) -> Option<PathBuf> {
    let (_, lib, _) = cache_slot(source, opts);
    lib.exists().then_some(lib)
}

/// Whether C compiler `cc` exists: a file if it is a path, else a program
/// in a `PATH` directory.
pub fn compiler_available(cc: &str) -> bool {
    let p = Path::new(cc);
    if p.components().count() > 1 {
        return p.is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(cc).is_file()))
}

/// The compiler's diagnostics in one line: the first line that reports an
/// error (else the first nonempty one), and how many lines follow.
fn first_error(diagnostics: &str) -> String {
    let lines: Vec<&str> = diagnostics.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let Some(first) = lines.iter().find(|l| l.contains("error")).or(lines.first()) else {
        return "no diagnostics".into();
    };
    match lines.len() - 1 {
        0 => first.to_string(),
        n => format!("{first} (and {n} more lines)"),
    }
}

/// Mark cache slot `dir` as used now (the least recently used slots are
/// pruned first, see `prune`).
fn touch(dir: &Path) {
    let _ = File::open(dir).and_then(|f| f.set_modified(SystemTime::now()));
}

/// Slots used more recently than this are never pruned (another run may be
/// about to load their library).
const PRUNE_KEEP_RECENT: Duration = Duration::from_secs(600);

/// Keep the cache within `TLX_CACHE_MB` (0: no limit): remove the least
/// recently used slots (by the modification time of their directory, see
/// `touch`) until the rest fits, except `keep` (the slot being built),
/// slots used in the last `PRUNE_KEEP_RECENT` and slots whose build is
/// running (their lock is held). Runs in the background: a new library is
/// being built, which takes much longer.
fn prune(opts: &BuildOptions, keep: &Path) {
    let limit = tlx_base::knobs::Env::process().num("TLX_CACHE_MB").unwrap_or(0) << 20;
    if limit == 0 {
        return;
    }
    let (cache, keep) = (opts.cache_dir.clone(), keep.to_path_buf());
    // Best effort: missing a prune affects cache size, not child ownership.
    let _ = std::thread::Builder::new().name("tlx-cache-prune".into()).spawn(move || prune_cache(&cache, &keep, limit));
}

/// `prune` (synchronously): the number of slots removed.
pub fn prune_cache(cache: &Path, keep: &Path, limit: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(cache) else { return 0 };
    let now = SystemTime::now();
    let mut slots = Vec::new();
    let mut total = 0;
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else { continue };
        // (Only the slots: directories named by a 16-digit hash.)
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !meta.is_dir() || name.len() != 16 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let size: u64 = std::fs::read_dir(&path).map_or(0, |fs| fs.flatten().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum());
        total += size;
        let used = meta.modified().unwrap_or(now);
        slots.push((used, size, path));
    }
    if total <= limit {
        return 0;
    }
    slots.sort();
    let mut removed = 0;
    for (used, size, path) in slots {
        if total <= limit {
            break;
        }
        if path == keep || now.duration_since(used).unwrap_or_default() < PRUNE_KEEP_RECENT {
            continue;
        }
        // (Not while a build of the slot runs: it holds the lock.)
        let Ok(lock) = File::options().create(true).truncate(false).write(true).open(path.join("build.lock")) else { continue };
        if lock.try_lock().is_err() {
            continue;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            total = total.saturating_sub(size);
            removed += 1;
        }
    }
    removed
}

/// Compile `source` (a complete C translation unit that may `#include
/// "tlx_abi.h"`) and return the path of the shared library. Reuses a cached
/// build when the source, flags and ABI are unchanged; waits for a build of
/// the same library that another process (or thread) is running.
pub fn build(source: &str, opts: &BuildOptions) -> Result<PathBuf, String> {
    start(source, opts)?.wait()
}

/// A build of a library started by `start`.
pub struct Build {
    lib: PathBuf,
    state: State,
}

enum State {
    /// The library is in the cache.
    Done,
    /// Our compiler process: (child, its temporary source, its log).
    Running(Child, PathBuf, PathBuf),
    /// Another build of the library holds its lock (the lock file, the
    /// source and options to build it ourselves if that build fails).
    Elsewhere(File, String, BuildOptions),
}

/// Start building `source` with `opts` in the background (see `build`);
/// `Build::wait` waits for the library.
///
/// The compiler runs in a shell that moves the library into the cache
/// itself, writes its diagnostics to a file and holds the lock of the
/// library: a build still running when tlx exits (builds run in the
/// background) completes into the cache, and a concurrent tlx run waits for
/// it instead of compiling the same library again.
pub fn start(source: &str, opts: &BuildOptions) -> Result<Build, String> {
    let (dir, lib, flags) = cache_slot(source, opts);
    if lib.exists() {
        touch(&dir);
        return Ok(Build { lib, state: State::Done });
    }
    if !compiler_available(&opts.cc) {
        return Err(format!("no C compiler ({} not found)", opts.cc));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    // One build of the library at a time: its compiler process holds the
    // lock (an advisory lock on `build.lock`, released when the processes
    // holding it exit, however they end).
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("build.lock"))
        .map_err(|e| format!("cannot open the build lock in {}: {e}", dir.display()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(Build { lib, state: State::Elsewhere(lock, source.to_string(), opts.clone()) }),
        Err(TryLockError::Error(e)) => return Err(format!("cannot lock the build in {}: {e}", dir.display())),
    }
    // (Built by the previous holder of the lock.)
    if lib.exists() {
        return Ok(Build { lib, state: State::Done });
    }
    // Build from and into names unique to this call, then rename, so that
    // concurrent builds (from other processes or threads) are safe. The
    // renames are done by the shell that runs the compiler: a build still
    // running when tlx exits completes into the cache instead of leaving
    // temporary files behind. A failed build removes its temporary files
    // there too, for the same reason (its log is read and removed here).
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stem = format!("model.{}.{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    // (The header too: rewriting it in place would let a concurrent build
    // of the same source read it half written.)
    let header = dir.join(format!("{stem}.h"));
    std::fs::write(&header, crate::ABI_HEADER).map_err(|e| e.to_string())?;
    std::fs::rename(&header, dir.join("tlx_abi.h")).map_err(|e| e.to_string())?;
    let c_path = dir.join(format!("{stem}.c"));
    std::fs::write(&c_path, source).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{stem}.{}", dylib_ext()));
    let log = dir.join(format!("{stem}.log"));
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(r#"cc="$1"; shift; "$cc" "$@" 2>"$TLX_LOG" && mv -f "$TLX_TMP" "$TLX_LIB" && { mv -f "$TLX_SRC" "$TLX_KEEP" || true; rm -f "$TLX_LOG"; } || { rm -f "$TLX_TMP" "$TLX_SRC"; exit 1; }"#)
        .arg("sh")
        .arg(&opts.cc)
        .args(flags.split_whitespace())
        .args(["-std=c11", "-fPIC", "-shared", "-fno-strict-aliasing", "-Wno-unused"])
        .arg("-I")
        .arg(&dir)
        .arg("-o")
        .arg(&tmp)
        .arg(&c_path)
        .env("TLX_TMP", &tmp)
        .env("TLX_LIB", &lib)
        .env("TLX_SRC", &c_path)
        .env("TLX_LOG", &log)
        // Keep the source next to the library for inspection.
        .env("TLX_KEEP", dir.join("model.c"))
        // The shell and the compiler hold the lock through their standard
        // input, and have no pipes to tlx (which may exit first).
        .stdin(Stdio::from(lock))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match cmd.spawn() {
        Ok(child) => {
            prune(opts, &dir);
            Ok(Build { lib, state: State::Running(child, c_path, log) })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&c_path);
            Err(format!("cannot run {}: {e}", opts.cc))
        }
    }
}

impl Build {
    /// Wait for the build: the path of the library.
    pub fn wait(mut self) -> Result<PathBuf, String> {
        let lib = std::mem::take(&mut self.lib);
        match std::mem::replace(&mut self.state, State::Done) {
            State::Done => Ok(lib),
            State::Running(mut child, c_path, log) => {
                let status = child.wait().map_err(|e| format!("C compilation ({}): {e}", c_path.display()))?;
                let diagnostics = std::fs::read_to_string(&log).unwrap_or_default();
                let _ = std::fs::remove_file(&log);
                if !status.success() || !lib.exists() {
                    return Err(format!("C compilation failed ({}): {}", c_path.display(), first_error(&diagnostics)));
                }
                Ok(lib)
            }
            State::Elsewhere(lock, source, opts) => {
                lock.lock().map_err(|e| format!("cannot lock the build of {}: {e}", lib.display()))?;
                drop(lock);
                if lib.exists() {
                    return Ok(lib);
                }
                // (The other build failed: ours says why.)
                build(&source, &opts)
            }
        }
    }
}

/// A build dropped while its compiler runs completes into the cache in the
/// background (a thread reaps the process). If that thread cannot start,
/// dropping the build waits synchronously; it never kills the compiler.
impl Drop for Build {
    fn drop(&mut self) {
        if let State::Running(child, _, log) = std::mem::replace(&mut self.state, State::Done) {
            if let Err(e) =
                reap_child(child, log, |reap| tlx_base::stack::spawn(std::thread::Builder::new().name("tlx-cc-reap".into()), reap))
            {
                eprintln!("cannot start C compiler reaper: {e}; synchronous reap attempted");
            }
        }
    }
}

/// The closure owns this guard as a whole: a failed spawn drops the closure
/// and reaps synchronously, including when the thread wrapper never calls it.
struct CompilerReaper {
    child: Child,
    log: PathBuf,
}

impl Drop for CompilerReaper {
    fn drop(&mut self) {
        match self.child.wait() {
            Ok(_) => {
                // No caller will consume these diagnostics. Cleanup after a
                // successful reap is best effort (the shell may remove it too).
                let _ = std::fs::remove_file(&self.log);
            }
            Err(e) => eprintln!("cannot reap C compiler {}: {e}; preserving diagnostics at {}", self.child.id(), self.log.display()),
        }
    }
}

fn reap_child(
    child: Child,
    log: PathBuf,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<std::thread::JoinHandle<()>>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    let reaper = CompilerReaper { child, log };
    spawn(Box::new(move || drop(reaper)))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }

    fn child_and_log(name: &str, exit: i32) -> (Child, PathBuf) {
        let log = std::env::temp_dir().join(format!("tlx-reaper-{name}-{}.log", std::process::id()));
        std::fs::write(&log, "compiler diagnostics").unwrap();
        let child = Command::new("sh").args(["-c", &format!("exit {exit}")]).current_dir(std::env::temp_dir()).env_clear().spawn().unwrap();
        (child, log)
    }

    fn assert_reaped(pid: u32) {
        let mut status = 0;
        // SAFETY: pid identifies our child, status is writable, and options=0
        // waits only for that child. This also cleans up the negative fixture.
        let result = unsafe { waitpid(pid as i32, &mut status, 0) };
        assert_eq!(result, -1, "the reaper left child {pid} waitable");
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(10), "expected ECHILD");
    }

    #[test]
    fn rejected_reaper_spawn_reaps_child_and_removes_log() {
        for exit in [0, 7] {
            let (child, log) = child_and_log(&format!("rejected-{exit}"), exit);
            let pid = child.id();
            let result = reap_child(child, log.clone(), |_reap| Err(std::io::Error::other("injected spawn failure")));
            assert!(result.is_err());
            let cleaned = !log.exists();
            let _ = std::fs::remove_file(log);
            assert_reaped(pid);
            assert!(cleaned, "successful synchronous reap must remove the log");
        }
    }

    #[test]
    fn background_reaper_reaps_child_and_removes_log() {
        let (child, log) = child_and_log("background", 7);
        let pid = child.id();
        reap_child(child, log.clone(), |reap| std::thread::Builder::new().spawn(reap)).unwrap().join().unwrap();
        assert_reaped(pid);
        assert!(!log.exists());
    }

    #[test]
    fn failed_reap_preserves_diagnostics() {
        let (child, log) = child_and_log("wait-failed", 7);
        let mut status = 0;
        // SAFETY: this is our child; consuming its status injects ECHILD into
        // the reaper's wait without replacing the process implementation.
        assert_eq!(unsafe { waitpid(child.id() as i32, &mut status, 0) }, child.id() as i32);
        reap_child(child, log.clone(), |reap| std::thread::Builder::new().spawn(reap)).unwrap().join().unwrap();
        let diagnostics = std::fs::read_to_string(&log);
        let _ = std::fs::remove_file(log);
        assert_eq!(diagnostics.unwrap(), "compiler diagnostics");
    }
}
