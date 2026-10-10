//! The native-code cache stays within its size limit (`TLX_CACHE_MB`): the
//! least recently used slots go first; the slot being built, slots used in
//! the last minutes and other files are kept.

use std::fs::File;
use std::path::Path;
use std::time::{Duration, SystemTime};

use tlx_native::build::prune_cache;

/// A cache slot `name` holding `kib` KiB, last used `age` ago.
fn slot(cache: &Path, name: &str, kib: usize, age: Duration) {
    let dir = cache.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("model.c"), vec![b'x'; kib << 10]).unwrap();
    File::open(&dir).unwrap().set_modified(SystemTime::now() - age).unwrap();
}

#[test]
fn prune_removes_the_least_recently_used_slots() {
    let cache = std::env::temp_dir().join(format!("tlx-native-prune-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let hour = Duration::from_secs(3600);
    slot(&cache, "000000000000000a", 100, 5 * hour);
    slot(&cache, "000000000000000b", 100, 4 * hour);
    slot(&cache, "000000000000000c", 100, 3 * hour);
    slot(&cache, "000000000000000d", 100, 2 * hour);
    slot(&cache, "000000000000000e", 100, Duration::from_secs(10));
    // (Not a slot: never removed, not counted.)
    std::fs::create_dir_all(cache.join("other")).unwrap();
    std::fs::write(cache.join("other/big"), vec![0u8; 1 << 20]).unwrap();
    let keep = cache.join("000000000000000a");
    // 500 KiB in slots, limit 250 KiB: a (being built) and e (recent) stay;
    // b, c and d go (oldest first) until the rest fits.
    assert_eq!(prune_cache(&cache, &keep, 250 << 10), 3);
    let mut left: Vec<_> = std::fs::read_dir(&cache).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    left.sort();
    assert_eq!(left, ["000000000000000a", "000000000000000e", "other"]);
    // Within the limit: nothing to do.
    assert_eq!(prune_cache(&cache, &keep, 1 << 30), 0);
    let _ = std::fs::remove_dir_all(&cache);
}
