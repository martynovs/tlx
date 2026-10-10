//! Run a real, unguarded overflow in a separate process: the handler must
//! terminate without unwinding, so this cannot run inside the unit harness.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "android"))]
fn main() {
    use std::os::unix::process::ExitStatusExt;
    #[inline(never)]
    fn recurse(n: usize) -> usize {
        let pad = std::hint::black_box([n; 128]);
        if std::hint::black_box(n) == usize::MAX {
            return 0;
        }
        recurse(n + 1).wrapping_add(std::hint::black_box(pad)[3])
    }

    if let Some(mode) = std::env::args().nth(1).filter(|a| !a.starts_with('-')) {
        // Avoid core files from the intentional default-signal deaths.
        // SAFETY: the initialized limit is readable for this call, which
        // changes only this isolated child process's core-dump resource limit.
        unsafe {
            let limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            libc::setrlimit(libc::RLIMIT_CORE, &limit);
        }
        match mode.as_str() {
            "main" => {
                tlx_base::stack::install_current().unwrap();
                std::hint::black_box(recurse(0));
            }
            "eval" => {
                tlx_base::stack::run_on_big_stack(256 << 10, || std::hint::black_box(recurse(0)));
            }
            "scoped" => std::thread::scope(|scope| {
                tlx_base::stack::spawn_default(scope, || std::hint::black_box(recurse(0))).join().unwrap();
            }),
            "native-build" => {
                tlx_base::stack::spawn(std::thread::Builder::new().name("tlx-cc".into()).stack_size(256 << 10), || {
                    std::hint::black_box(recurse(0))
                })
                .unwrap()
                .join()
                .unwrap();
            }
            "parallel" => {
                let barrier = std::sync::Barrier::new(8);
                std::thread::scope(|scope| {
                    for id in 0..8 {
                        let barrier = &barrier;
                        tlx_base::stack::spawn_default(scope, move || {
                            barrier.wait();
                            if id == 7 {
                                std::hint::black_box(recurse(0));
                            }
                        });
                    }
                });
            }
            "segv" | "bus" | "unrelated" => {
                // SAFETY: this isolated fatal child owns any mapping below;
                // raise targets its own thread. The unrelated mode deliberately
                // violates the writable-address precondition of write_volatile
                // to provoke a protected-page OS fault. This negative fixture
                // is not a sound Rust access or a Miri target; only the isolated
                // child's signal death is observed, never continued execution.
                tlx_base::stack::run_on_big_stack(256 << 10, move || unsafe {
                    if mode == "unrelated" {
                        // A real protected-memory fault away from this
                        // thread's guard must retain SIGSEGV/SIGBUS death.
                        let mapping = libc::mmap(std::ptr::null_mut(), 16384, libc::PROT_NONE, libc::MAP_PRIVATE | libc::MAP_ANON, -1, 0);
                        assert_ne!(mapping, libc::MAP_FAILED);
                        std::ptr::write_volatile(mapping.cast::<u8>(), 1);
                    } else {
                        libc::raise(if mode == "segv" { libc::SIGSEGV } else { libc::SIGBUS });
                    }
                });
            }
            _ => panic!("unknown child mode: {mode}"),
        }
        panic!("fatal child returned");
    }
    for mode in ["main", "eval", "scoped", "native-build", "parallel", "segv", "bus", "unrelated"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap()).arg(mode).output().unwrap();
        let stderr = String::from_utf8(result.stderr).unwrap();
        if matches!(mode, "segv" | "bus" | "unrelated") {
            let signal = result.status.signal().expect("unrelated fault must die by signal");
            if mode == "unrelated" {
                assert!(matches!(signal, libc::SIGSEGV | libc::SIGBUS), "{mode}: {signal}");
            } else {
                assert_eq!(signal, if mode == "segv" { libc::SIGSEGV } else { libc::SIGBUS });
            }
            assert!(!stderr.contains(tlx_base::stack::OVERFLOW_MESSAGE), "{mode}: {stderr}");
        } else {
            assert_eq!(result.status.code(), Some(tlx_base::stack::OVERFLOW_EXIT.into()), "{mode}: {stderr}");
            assert_eq!(stderr, format!("Error: {}\n", tlx_base::stack::OVERFLOW_MESSAGE), "{mode}");
        }
        println!("{mode}: passed");
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
fn main() {}
