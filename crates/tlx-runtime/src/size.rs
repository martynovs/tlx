//! The size-string grammar shared by the memory and disk option parsers
//! (`-maxmem`/`-maxdisk`, docs/configuration.md): decimal digits followed by
//! an optional power-of-1024 suffix.
//!
//! Only the grammar lives here. The two parsers keep their own value
//! domains: `mem::parse_budget` maps to `usize` bytes, also accepts a
//! fraction of the physical memory and floors at 1 MiB, while
//! `io::parse_size` maps to a positive `u64` byte count.

/// Split `v` at the end of its leading ASCII-digit run: the digits and
/// everything after them (the suffix, possibly empty and possibly padded
/// with whitespace).
pub(crate) fn digits_and_suffix(v: &str) -> (&str, &str) {
    let digits = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
    v.split_at(digits)
}

/// The power-of-1024 shift, in bits, named by a size suffix: `K`/`KB`/`KiB`
/// (2^10), `M`/`MB`/`MiB` (2^20), `G`/`GB`/`GiB` (2^30) or `T`/`TB`/`TiB`
/// (2^40); no suffix (or only whitespace) is MiB. Case does not matter and
/// the suffix may be padded with whitespace; anything else is `None`.
pub(crate) fn shift_of(suffix: &str) -> Option<u32> {
    match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "m" | "mb" | "mib" => Some(20),
        "k" | "kb" | "kib" => Some(10),
        "g" | "gb" | "gib" => Some(30),
        "t" | "tb" | "tib" => Some(40),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::parse_size;
    use crate::mem::{parse_budget, physical_bytes};

    #[test]
    fn suffix_table() {
        for (suffix, shift) in [
            ("", 20),
            (" ", 20),
            ("m", 20),
            ("M", 20),
            ("mb", 20),
            ("MB", 20),
            ("MiB", 20),
            ("mib", 20),
            ("k", 10),
            ("K", 10),
            ("kB", 10),
            ("KiB", 10),
            ("kib", 10),
            ("g", 30),
            ("G", 30),
            ("gb", 30),
            ("GiB", 30),
            ("gib", 30),
            ("t", 40),
            ("T", 40),
            ("tb", 40),
            ("TiB", 40),
            ("tib", 40),
            ("  G  ", 30),
            ("\tG", 30),
            ("M ", 20),
        ] {
            assert_eq!(shift_of(suffix), Some(shift), "{suffix:?}");
        }
        for bad in ["b", "x", "8", "12", "e3", "mib2", "M G", "kmb", "MIBB", "．"] {
            assert_eq!(shift_of(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn digits_split() {
        assert_eq!(digits_and_suffix(""), ("", ""));
        assert_eq!(digits_and_suffix("8"), ("8", ""));
        assert_eq!(digits_and_suffix("512MiB"), ("512", "MiB"));
        assert_eq!(digits_and_suffix("8 G "), ("8", " G "));
        assert_eq!(digits_and_suffix("x8"), ("", "x8"));
        assert_eq!(digits_and_suffix("-8"), ("", "-8"));
        assert_eq!(digits_and_suffix("+8"), ("", "+8"));
        assert_eq!(digits_and_suffix(".5"), ("", ".5"));
        // A non-ASCII digit is not an ASCII digit: the split stays on a
        // character boundary and the numeric part stays empty.
        let (digits, suffix) = digits_and_suffix("８0");
        assert_eq!((digits, suffix), ("", "８0"));
    }

    /// The old/new table of both parsers (values captured from the
    /// pre-refactor implementation of `io::parse_size` and
    /// `mem::parse_budget`): same spellings, same case/space handling, same
    /// zero, minimum and overflow answers; the value domains differ as
    /// documented (the disk parser has no 1 MiB floor and takes K).
    #[test]
    fn old_new_table() {
        let table: &[(&str, Option<u64>, Option<usize>)] = &[
            ("", None, None),
            (" ", None, None),
            ("0", None, None),
            ("0K", None, None),
            ("0M", None, None),
            ("1", Some(1 << 20), Some(1 << 20)),
            ("512", Some(512 << 20), Some(512 << 20)),
            ("512M", Some(512 << 20), Some(512 << 20)),
            ("512MB", Some(512 << 20), Some(512 << 20)),
            ("512MiB", Some(512 << 20), Some(512 << 20)),
            ("512mib", Some(512 << 20), Some(512 << 20)),
            ("512 MB", Some(512 << 20), Some(512 << 20)),
            ("8G", Some(8 << 30), Some(8 << 30)),
            ("8g", Some(8 << 30), Some(8 << 30)),
            ("8GB", Some(8 << 30), Some(8 << 30)),
            ("8gib", Some(8 << 30), Some(8 << 30)),
            ("2T", Some(2 << 40), Some(2 << 40)),
            ("2tb", Some(2 << 40), Some(2 << 40)),
            // The domain difference: the disk parser takes kibibytes and has
            // no floor; the memory parser floors at 1 MiB.
            ("64k", Some(64 << 10), None),
            ("64KB", Some(64 << 10), None),
            ("1K", Some(1 << 10), None),
            ("100K", Some(100 << 10), None),
            ("1023K", Some(1023 << 10), None),
            ("1024K", Some(1 << 20), Some(1 << 20)),
            ("1M", Some(1 << 20), Some(1 << 20)),
            ("1m", Some(1 << 20), Some(1 << 20)),
            // Space and case.
            (" 8 G ", Some(8 << 30), Some(8 << 30)),
            ("8\tG", Some(8 << 30), Some(8 << 30)),
            ("  8", Some(8 << 20), Some(8 << 20)),
            ("8  ", Some(8 << 20), Some(8 << 20)),
            (" 8gib ", Some(8 << 30), Some(8 << 30)),
            // Rejected spellings.
            ("8.5M", None, None),
            ("0.5M", None, None),
            ("1.5", None, None),
            ("0.0", None, None),
            ("1.0", None, None),
            ("1e3", None, None),
            ("-1", None, None),
            ("+1", None, None),
            ("x", None, None),
            ("b", None, None),
            ("8b", None, None),
            ("8Q", None, None),
            ("8Mx", None, None),
            ("8 M G", None, None),
            ("1,5", None, None),
            ("8_0", None, None),
            ("0x10", None, None),
            ("1 2", None, None),
            ("1M2", None, None),
            ("inf", None, None),
            ("nan", None, None),
            ("infinity", None, None),
            ("-inf", None, None),
            ("８", None, None),
            ("8٨", None, None),
            // Overflow of the digit run or of the multiplication, each
            // checked in the wrapper's own width (u64 disk, usize memory).
            ("18446744073709551615", None, None),
            ("18446744073709551616", None, None),
            ("18446744073709551615M", None, None),
            ("18446744073709551615T", None, None),
            ("99999999999999999999999999", None, None),
            ("16777216T", None, None),
            ("17179869184G", None, None),
            ("9007199254740992M", None, None),
        ];
        for (v, disk, mem) in table {
            assert_eq!(parse_size(v), *disk, "parse_size({v:?})");
            assert_eq!(parse_budget(v), *mem, "parse_budget({v:?})");
        }
    }

    /// The fractional branch stays with the memory parser: a fraction in
    /// (0,1) of the physical memory, floored at 1 MiB, and never a size
    /// suffix.
    #[test]
    fn memory_fraction() {
        let phys = physical_bytes() as f64;
        assert_eq!(parse_budget("0.5"), Some(physical_bytes() / 2));
        assert_eq!(parse_budget("0.5 "), Some(physical_bytes() / 2));
        assert_eq!(parse_budget("0.25"), Some(physical_bytes() / 4));
        assert_eq!(parse_budget(".5"), Some(physical_bytes() / 2));
        assert_eq!(parse_budget("+0.5"), Some(physical_bytes() / 2));
        assert_eq!(parse_budget("0.000001"), Some(1 << 20));
        let almost = parse_budget("0.999999999").unwrap();
        assert!(almost > physical_bytes() / 2 && almost <= phys as usize, "{almost}");
        // `-maxdisk` has no fractional form.
        assert_eq!(parse_size("0.5"), None);
    }
}
